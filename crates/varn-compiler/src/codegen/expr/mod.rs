mod assignment;
mod calls;
mod collections;
mod fields;
mod member;
mod operators;
mod templates;

pub use calls::compile_args_contiguous;
pub use fields::emit_field_inits;

use super::compiler::Compiler;
use super::function::{compile_function, emit_closure};
use crate::chunk::{Chunk, Literal, PoolEntry};
use std::rc::Rc;
use varn_core::ast::expr::ArrowBody;
use varn_core::ast::{Expr, ExprKind, Stmt, StmtKind};
use varn_core::{well_known as wk, OpCode};

pub fn compile_expr<'a>(c: &mut Compiler<'a>, expr: &Expr) -> u8 {
    c.line = expr.range.start.line;
    match &expr.kind {
        ExprKind::NullLiteral => {
            let r = c.alloc_reg();
            c.emit_rr(OpCode::LoadNull, r, 0);

            let vreg = c.emit_ir_unary(OpCode::LoadNull, super::ir::ImmValue::None);
            c.map_phys_to_vreg(r, vreg);
            r
        }
        ExprKind::BoolLiteral { value } => {
            let r = c.alloc_reg();
            let op = if *value {
                OpCode::LoadTrue
            } else {
                OpCode::LoadFalse
            };
            c.emit_rr(op, r, 0);

            let vreg = c.emit_ir_unary(op, super::ir::ImmValue::None);
            c.map_phys_to_vreg(r, vreg);
            r
        }
        ExprKind::IntLiteral { value, .. } => {
            let r = c.alloc_reg();
            c.emit_load_int(r, *value);

            let imm = if *value >= i16::MIN as i64 && *value <= i16::MAX as i64 {
                super::ir::ImmValue::Small(*value as i16)
            } else {
                super::ir::ImmValue::Large(*value as i32)
            };
            let vreg = c.emit_ir_unary(OpCode::LoadInt, imm);
            c.map_phys_to_vreg(r, vreg);
            r
        }
        ExprKind::FloatLiteral { value, .. } => {
            let idx = c.add_const(PoolEntry::Literal(Literal::Float(*value)));
            let r = c.alloc_reg();
            c.emit_rc(OpCode::LoadConst, r, idx);

            let vreg = c.emit_ir_unary(OpCode::LoadConst, super::ir::ImmValue::Index(idx as u32));
            c.map_phys_to_vreg(r, vreg);
            r
        }
        ExprKind::BigIntLiteral { raw } => {
            let s = raw.trim_end_matches('n').replace('_', "");
            let parsed = if let Some(r) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                i128::from_str_radix(r, 16)
            } else if let Some(r) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
                i128::from_str_radix(r, 8)
            } else if let Some(r) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
                i128::from_str_radix(r, 2)
            } else {
                s.parse()
            };
            let num = match parsed {
                Ok(n) => n,
                Err(_) => {
                    c.set_error(format!("bigint literal overflow: {raw}"));
                    0i128
                }
            };
            let idx = c.add_const(PoolEntry::Literal(Literal::BigInt(num)));
            let r = c.alloc_reg();
            c.emit_rc(OpCode::LoadConst, r, idx);

            let vreg = c.emit_ir_unary(OpCode::LoadConst, super::ir::ImmValue::Index(idx as u32));
            c.map_phys_to_vreg(r, vreg);
            r
        }
        ExprKind::DecimalLiteral { raw } => {
            use std::str::FromStr;
            let d = rust_decimal::Decimal::from_str(raw.trim_end_matches('d'))
                .unwrap_or(rust_decimal::Decimal::ZERO);
            let idx = c.add_const(PoolEntry::Literal(Literal::Decimal(d)));
            let r = c.alloc_reg();
            c.emit_rc(OpCode::LoadConst, r, idx);

            let vreg = c.emit_ir_unary(OpCode::LoadConst, super::ir::ImmValue::Index(idx as u32));
            c.map_phys_to_vreg(r, vreg);
            r
        }
        ExprKind::StrLiteral { value } => {
            let idx = c.add_str(value);
            let r = c.alloc_reg();
            c.emit_rc(OpCode::LoadConst, r, idx);

            let vreg = c.emit_ir_unary(OpCode::LoadConst, super::ir::ImmValue::Index(idx as u32));
            c.map_phys_to_vreg(r, vreg);
            r
        }
        ExprKind::CharLiteral { value } => {
            let idx = c.add_const(PoolEntry::Literal(Literal::Char(*value)));
            let r = c.alloc_reg();
            c.emit_rc(OpCode::LoadConst, r, idx);

            let vreg = c.emit_ir_unary(OpCode::LoadConst, super::ir::ImmValue::Index(idx as u32));
            c.map_phys_to_vreg(r, vreg);
            r
        }
        ExprKind::RegexLiteral { pattern, flags } => {
            let raw = format!("/{pattern}/{flags}");
            let idx = c.add_str(&raw);
            let r = c.alloc_reg();
            c.emit_rc(OpCode::LoadConst, r, idx);
            r
        }

        ExprKind::Identifier { name } => {
            let dest = c.alloc_reg();
            if !c.emit_load_var(name, dest) {
                let idx = c.add_str(name);
                c.emit_rc(OpCode::LoadGlobal, dest, idx);
            }
            dest
        }
        ExprKind::This => {
            let dest = c.alloc_reg();
            if !c.emit_load_var("this", dest) {
                let idx = c.add_str("this");
                c.emit_rc(OpCode::LoadGlobal, dest, idx);
            }
            dest
        }
        ExprKind::Super => {
            let dest = c.alloc_reg();
            let idx = c.add_str("super");
            c.emit_rc(OpCode::GetSuper, dest, idx);
            dest
        }

        ExprKind::Paren { expression } => compile_expr(c, expression),
        ExprKind::As { expression, .. } => compile_expr(c, expression),
        ExprKind::Satisfies { expression, .. } => compile_expr(c, expression),
        ExprKind::NonNull { expression } => {
            let r = compile_expr(c, expression);
            let line = c.line;
            c.chunk
                .emit1(OpCode::AssertNotNull, Chunk::pack(0, r) as u16, line);
            r
        }
        ExprKind::Try { expression } => operators::compile_try_expr(c, expression),

        ExprKind::Unary { op, operand, .. } => operators::compile_unary(c, op, operand),

        ExprKind::Update {
            op,
            prefix,
            operand,
        } => assignment::compile_update(c, op, *prefix, operand),

        ExprKind::Binary { op, left, right } => {
            operators::compile_binary(c, op, left, right, expr.range.start.offset)
        }

        ExprKind::Logical { op, left, right } => operators::compile_logical(c, op, left, right),

        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => operators::compile_conditional(c, test, consequent, alternate),

        ExprKind::Assign { op, target, value } => assignment::compile_assign(c, op, target, value),

        ExprKind::Member {
            object,
            property,
            computed,
            optional,
        } => member::compile_member_access(
            c,
            object,
            property,
            *computed,
            *optional,
            expr.range.start.offset,
        ),

        ExprKind::Call {
            callee,
            args,
            optional,
            ..
        } => calls::compile_call(c, callee, args, *optional, expr.range.start.offset),

        ExprKind::New { callee, args, .. } => {
            let callee_reg = compile_expr(c, callee);
            let line = c.line;
            let recv_reg = c.alloc_reg();
            c.chunk.emit_rr(OpCode::LoadNull, recv_reg, 0, line);

            let (arg_start, arg_count, has_spread) =
                calls::compile_args_contiguous(c, expr.range.start.offset, args);
            assert_eq!(
                arg_start,
                recv_reg + 1,
                "contiguous args must start right after receiver register"
            );

            let dest = callee_reg;
            if has_spread {
                c.chunk.emit(OpCode::CallSpread, line);
                c.chunk.write(Chunk::pack(dest, callee_reg), line);
                c.chunk
                    .write(Chunk::pack((arg_count + 1) as u8, recv_reg), line);
            } else {
                c.chunk.emit(OpCode::Call, line);
                c.chunk.write(Chunk::pack(dest, callee_reg), line);
                c.chunk
                    .write(Chunk::pack((arg_count + 1) as u8, recv_reg), line);
            }
            for _ in 0..arg_count {
                c.free_reg();
            }
            c.free_reg();
            dest
        }

        ExprKind::Array { elements, .. } => collections::compile_array(c, elements),

        ExprKind::Object { properties, .. } => collections::compile_object(c, properties),

        ExprKind::Template { parts } => templates::compile_template(c, parts),

        ExprKind::TaggedTemplate { tag, template, .. } => {
            let tag_reg = compile_expr(c, tag);
            let tpl_reg = compile_expr(c, template);
            let dest = c.alloc_reg();
            let line = c.line;
            c.chunk.emit(OpCode::Call, line);
            c.chunk.write(Chunk::pack(dest, tag_reg), line);
            c.chunk.write(Chunk::pack(1, tpl_reg), line);
            c.free_reg();
            c.free_reg();
            dest
        }

        ExprKind::Function {
            fn_id,
            params,
            body,
            is_async,
            is_generator,
            ..
        } => {
            let name = fn_id.clone().unwrap_or_else(|| Rc::from("<anonymous>"));
            let (proto, upvalues) =
                compile_function(c, name, params, body, *is_async, *is_generator, false);
            emit_closure(c, proto, upvalues, expr.id)
        }
        ExprKind::Arrow {
            params,
            body,
            is_async,
            ..
        } => {
            let stmt_body = match body.as_ref() {
                ArrowBody::Block(s) => s.clone(),
                ArrowBody::Expr(e) => Stmt::new_with_range(
                    e.range,
                    StmtKind::Return {
                        argument: Some(Box::new(e.clone())),
                    },
                ),
            };
            let (proto, upvalues) = compile_function(
                c,
                Rc::from("<arrow>"),
                params,
                &stmt_body,
                *is_async,
                false,
                false,
            );
            emit_closure(c, proto, upvalues, expr.id)
        }
        ExprKind::ClassExpr { declaration } => super::class::compile_class_expr(c, declaration),

        ExprKind::Await { argument } => {
            let src = compile_expr(c, argument);
            let dest = c.alloc_reg();
            c.emit_rr(OpCode::Await, dest, src);
            c.free_reg();
            dest
        }
        ExprKind::Spawn { argument } => {
            let src = compile_expr(c, argument);
            let dest = c.alloc_reg();
            let line = c.line;
            c.chunk.emit(OpCode::Spawn, line);
            c.chunk.write(Chunk::pack(dest, src), line);
            c.chunk.write(Chunk::pack(0, 0), line);
            c.free_reg();
            dest
        }
        ExprKind::Yield { argument, .. } => {
            let src = if let Some(val) = argument {
                compile_expr(c, val)
            } else {
                let r = c.alloc_reg();
                c.emit_rr(OpCode::LoadNull, r, 0);
                r
            };
            let dest = c.alloc_reg();
            let line = c.line;

            c.chunk
                .emit1(OpCode::Yield, Chunk::pack(dest, src) as u16, line);
            c.free_reg();
            dest
        }

        ExprKind::Spread { argument } => {
            let src = compile_expr(c, argument);
            let dest = c.alloc_reg();
            c.emit_rr(OpCode::WrapSpread, dest, src);
            c.free_reg();
            dest
        }

        ExprKind::Sequence { expressions } => {
            if expressions.is_empty() {
                let r = c.alloc_reg();
                c.emit_rr(OpCode::LoadNull, r, 0);
                return r;
            }
            let mut last = 0u8;
            for (i, e) in expressions.iter().enumerate() {
                let r = compile_expr(c, e);
                if i + 1 < expressions.len() {
                    c.free_reg();
                } else {
                    last = r;
                }
            }
            last
        }

        ExprKind::Range {
            start,
            end,
            inclusive,
        } => {
            let s = compile_expr(c, start);
            let e = compile_expr(c, end);
            let dest = c.alloc_reg();
            let flag = if *inclusive { 1u16 } else { 0u16 };
            let method = c.add_str(wk::RUNTIME_RANGE);
            let line = c.line;

            c.chunk.emit(OpCode::InvokeRuntimeStatic, line);
            c.chunk.write(Chunk::pack(dest, 0), line);
            c.chunk.write(method, line);
            c.chunk.write(Chunk::pack(2, s), line);
            c.chunk.write(Chunk::pack(e as u8, flag as u8), line);
            c.free_reg();
            c.free_reg();
            dest
        }

        ExprKind::Pipeline { left, right } => templates::compile_pipeline(c, left, right),

        ExprKind::Match { subject, cases } => templates::compile_match(c, subject, cases),

        ExprKind::Is {
            expression,
            type_ann,
        } => member::compile_is(c, expression, type_ann),
    }
}
