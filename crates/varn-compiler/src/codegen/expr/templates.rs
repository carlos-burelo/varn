use super::super::compiler::Compiler;
use crate::chunk::Chunk;
use std::rc::Rc;
use varn_core::ast::expr::{Arg, MatchCase, TemplatePart};
use varn_core::ast::operators::Modifiers;
use varn_core::ast::pattern::MatchPattern;
use varn_core::ast::{ArrayEl, Expr, ExprKind, MatchBody, Param, Pattern, Stmt, StmtKind};
use varn_core::OpCode;

use super::super::function::{compile_function, emit_closure};
use super::compile_expr;

pub(super) fn compile_template<'a>(c: &mut Compiler<'a>, parts: &[TemplatePart]) -> u8 {
    if parts.is_empty() {
        let r = c.alloc_reg();
        let idx = c.add_str("");
        c.emit_rc(OpCode::LoadConst, r, idx);
        return r;
    }

    if parts.len() == 1 {
        if let TemplatePart::Literal(s) = &parts[0] {
            let r = c.alloc_reg();
            let idx = c.add_str(s);
            c.emit_rc(OpCode::LoadConst, r, idx);
            return r;
        }
    }

    let part_regs: Vec<u8> = parts
        .iter()
        .map(|part| match part {
            TemplatePart::Literal(s) => {
                let r = c.alloc_reg();
                let idx = c.add_str(s);
                c.emit_rc(OpCode::LoadConst, r, idx);
                r
            }
            TemplatePart::Interpolation(e) => {
                let r = compile_expr(c, e);
                c.emit_rr(OpCode::ToString, r, r);
                r
            }
        })
        .collect();

    let dest = c.alloc_reg();
    let count = part_regs.len() as u8;
    let line = c.line;
    c.chunk
        .write(crate::chunk::Chunk::pack_op(OpCode::BuildStr, dest), line);
    c.chunk.write(crate::chunk::Chunk::pack(count, 0), line);
    for &reg in &part_regs {
        c.chunk.write(crate::chunk::Chunk::pack(reg, 0), line);
    }

    for _ in &part_regs {
        c.free_reg();
    }

    dest
}

pub(super) fn compile_pipeline<'a>(c: &mut Compiler<'a>, left: &Expr, right: &Expr) -> u8 {
    let fn_reg = if pipeline_has_placeholder(right) {
        let range = varn_core::SourceRange::default();
        let param = Param {
            pattern: Pattern::Identifier {
                name: Rc::from("_"),
                type_ann: None,
                range,
            },
            type_ann: None,
            default: None,
            is_rest: false,
            is_optional: false,
            modifiers: Modifiers::default(),
            range,
        };
        let body = Stmt::new_with_range(
            range,
            StmtKind::Return {
                argument: Some(Box::new(right.clone())),
            },
        );
        let (proto, upvalues) =
            compile_function(c, Rc::from("<pipe>"), &[param], &body, false, false, false);
        emit_closure(c, proto, upvalues, 0)
    } else {
        compile_expr(c, right)
    };

    let line = c.line;
    let recv_reg = c.alloc_reg();
    c.chunk.emit_rr(OpCode::LoadNull, recv_reg, 0, line);

    let arg = compile_expr(c, left);
    assert_eq!(
        arg,
        recv_reg + 1,
        "contiguous args must start right after receiver register"
    );

    c.chunk.emit(OpCode::Call, line);
    c.chunk.write(Chunk::pack(fn_reg, fn_reg), line);
    c.chunk.write(Chunk::pack(2, recv_reg), line);
    c.free_reg();
    c.free_reg();
    fn_reg
}

fn pipeline_has_placeholder(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Identifier { name } => &**name == "_",
        ExprKind::Call { callee, args, .. } => {
            pipeline_has_placeholder(callee)
                || args.iter().any(|a| match a {
                    Arg::Positional(e) | Arg::Spread(e) => pipeline_has_placeholder(e),
                    Arg::Named { value, .. } => pipeline_has_placeholder(value),
                })
        }
        ExprKind::Member { object, .. } => pipeline_has_placeholder(object),
        ExprKind::Paren { expression } => pipeline_has_placeholder(expression),
        ExprKind::Binary { left, right, .. } | ExprKind::Logical { left, right, .. } => {
            pipeline_has_placeholder(left) || pipeline_has_placeholder(right)
        }
        ExprKind::Unary { operand, .. } => pipeline_has_placeholder(operand),
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            pipeline_has_placeholder(test)
                || pipeline_has_placeholder(consequent)
                || pipeline_has_placeholder(alternate)
        }
        ExprKind::Array { elements, .. } => elements.iter().any(|el| match el {
            ArrayEl::Expr(e) | ArrayEl::Spread(e) => pipeline_has_placeholder(e),
            ArrayEl::Hole => false,
        }),
        _ => false,
    }
}

pub(super) fn compile_match<'a>(c: &mut Compiler<'a>, subject: &Expr, cases: &[MatchCase]) -> u8 {
    let subj = compile_expr(c, subject);
    let dest = c.alloc_reg();
    c.emit_rr(OpCode::LoadNull, dest, 0);
    let mut end_jumps: Vec<usize> = vec![];

    for case in cases {
        c.push_scope();
        let saved = c.regs.save();

        let skip: Option<usize> = match &case.pattern {
            MatchPattern::Wildcard => {
                c.define_local(Rc::from("__match_subj__"), subj);
                None
            }
            MatchPattern::Literal(lit) => {
                let lit_r = compile_expr(c, lit);
                let eq_r = c.alloc_reg();
                c.emit_rrr(OpCode::Eq, eq_r, subj, lit_r);
                c.free_reg();
                let j = c.emit_cond_jump(OpCode::JumpIfFalse, eq_r);
                c.free_reg();
                c.define_local(Rc::from("__match_subj__"), subj);
                Some(j)
            }
            MatchPattern::Identifier(name) => {
                let bind_r = c.alloc_reg();
                c.emit_rr(OpCode::Move, bind_r, subj);
                c.define_local(name.clone(), bind_r);
                None
            }
            MatchPattern::Record { fields, .. } => {
                c.define_local(Rc::from("__match_subj__"), subj);
                for (field_name, sub_pat) in fields {
                    let fkey = c.add_str(field_name);
                    let field_r = c.alloc_reg();
                    c.emit_property(OpCode::GetProperty, field_r, subj, fkey);
                    let binding = match sub_pat {
                        Some(MatchPattern::Identifier(n)) => n.clone(),
                        _ => field_name.clone(),
                    };
                    c.define_local(binding, field_r);
                }
                None
            }
            MatchPattern::EnumVariant {
                variant_name,
                bindings,
                ..
            } => {
                let tag_key = c.add_str("__variant_name__");
                let tag_r = c.alloc_reg();
                c.emit_property(OpCode::GetProperty, tag_r, subj, tag_key);
                let vname_idx = c.add_str(variant_name);
                let vname_r = c.alloc_reg();
                c.emit_rc(OpCode::LoadConst, vname_r, vname_idx);
                let eq_r = c.alloc_reg();
                c.emit_rrr(OpCode::Eq, eq_r, tag_r, vname_r);
                c.free_reg();
                c.free_reg();
                let j = c.emit_cond_jump(OpCode::JumpIfFalse, eq_r);
                c.free_reg();
                c.define_local(Rc::from("__match_subj__"), subj);
                for (i, binding) in bindings.iter().enumerate() {
                    let fkey = c.add_str(&format!("value{i}"));
                    let field_r = c.alloc_reg();
                    c.emit_property(OpCode::GetProperty, field_r, subj, fkey);
                    if &*binding.name != "_" {
                        c.define_local(binding.name.clone(), field_r);
                    } else {
                        c.free_reg();
                    }
                }
                Some(j)
            }
            _ => None,
        };

        let body_r = match &case.body {
            MatchBody::Block(s) => {
                super::super::stmt::compile_stmt(c, s);
                let r = c.alloc_reg();
                c.emit_rr(OpCode::LoadNull, r, 0);
                r
            }
            MatchBody::Expr(e) => compile_expr(c, e),
        };
        c.emit_rr(OpCode::Move, dest, body_r);
        c.free_reg();

        let end = c.emit_jump(OpCode::Jump);
        end_jumps.push(end);

        if let Some(s) = skip {
            c.patch_jump(s);
        }

        c.regs.restore(saved);
        c.pop_scope();
    }

    c.free_reg();
    for j in end_jumps {
        c.patch_jump(j);
    }
    dest
}
