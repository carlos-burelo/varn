use super::compiler::Compiler;
use super::function::{compile_function, emit_closure};
use crate::chunk::{Chunk, Literal, PoolEntry};
use std::rc::Rc;
use varn_core::ast::expr::{Arg, ArrowBody, ObjectProp, PropKey, TemplatePart};
use varn_core::ast::operators::{AssignOp, BinaryOp, LogicalOp, Modifiers, UnaryOp, UpdateOp};
use varn_core::ast::pattern::MatchPattern;
use varn_core::ast::{ArrayEl, Expr, ExprKind, MatchBody, Param, Pattern, Stmt, StmtKind};
use varn_core::{well_known as wk, MemberKey, OpCode};

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
            let num: i128 = if let Some(r) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                i128::from_str_radix(r, 16)
                    .unwrap_or_else(|_| panic!("bigint literal overflow: {raw}"))
            } else if let Some(r) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
                i128::from_str_radix(r, 8)
                    .unwrap_or_else(|_| panic!("bigint literal overflow: {raw}"))
            } else if let Some(r) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
                i128::from_str_radix(r, 2)
                    .unwrap_or_else(|_| panic!("bigint literal overflow: {raw}"))
            } else {
                s.parse().unwrap_or_else(|_| panic!("bigint literal overflow: {raw}"))
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
        ExprKind::Try { expression } => {
            let r = compile_expr(c, expression);
            let tag = c.alloc_reg();
            c.emit_rr(OpCode::GetEnumTag, tag, r);
            let ok = c.emit_cond_jump(OpCode::JumpIfTrue, tag);
            c.free_reg();

            let ret_null = c.alloc_reg();
            c.emit_rr(OpCode::LoadNull, ret_null, 0);
            let line = c.line;
            c.chunk
                .emit1(OpCode::Return, Chunk::pack(0, r) as u16, line);
            c.free_reg();
            c.patch_jump(ok);
            r
        }

        ExprKind::Unary { op, operand, .. } => {
            let src = compile_expr(c, operand);
            match op {
                UnaryOp::Minus => c.emit_rr(OpCode::Negate, src, src),
                UnaryOp::Plus => {}
                UnaryOp::Not => c.emit_rr(OpCode::Not, src, src),
                UnaryOp::BitNot => {
                    let neg1_idx = c.add_const(PoolEntry::Literal(Literal::Int(-1)));
                    let neg1 = c.alloc_reg();
                    c.emit_rc(OpCode::LoadConst, neg1, neg1_idx);
                    c.emit_rrr(OpCode::BitXor, src, src, neg1);
                    c.free_reg();
                }
                UnaryOp::Typeof => c.emit_rr(OpCode::Typeof, src, src),
            }

            if let Some(vreg_src) = c.get_vreg_for_phys(src) {
                let opcode_ir = match op {
                    UnaryOp::Minus => OpCode::Negate,
                    UnaryOp::Plus => return src,
                    UnaryOp::Not => OpCode::Not,
                    UnaryOp::BitNot => OpCode::BitXor,
                    UnaryOp::Typeof => OpCode::Typeof,
                };
                let vreg_dest = c.emit_ir_unary_src(opcode_ir, vreg_src);
                c.map_phys_to_vreg(src, vreg_dest);
            }

            src
        }

        ExprKind::Update {
            op,
            prefix,
            operand,
        } => compile_update(c, op, *prefix, operand),

        ExprKind::Binary { op, left, right } => {
            if let Some(folded) = try_fold_binary(c, op, left, right) {
                return folded;
            }
            if let Some(r) = try_emit_imm_op(c, op, left, right) {
                return r;
            }
             let l = compile_expr(c, left);
             let r = compile_expr(c, right);
             let numeric_kind = c.annotations.get_numeric(expr.range.start.offset);
             let opcode = match numeric_kind {
                 Some(varn_core::NumericKind::Int) => match op {
                     BinaryOp::Add => OpCode::AddInt,
                     BinaryOp::Sub => OpCode::SubInt,
                     BinaryOp::Mul => OpCode::MulInt,
                     BinaryOp::Div => OpCode::DivInt,
                     BinaryOp::Eq => OpCode::EqInt,
                     BinaryOp::NotEq => OpCode::NeqInt,
                     BinaryOp::Lt => OpCode::LtInt,
                     BinaryOp::LtEq => OpCode::LteInt,
                     BinaryOp::Gt => OpCode::GtInt,
                     BinaryOp::GtEq => OpCode::GteInt,
                     BinaryOp::Mod => OpCode::Mod,
                     BinaryOp::Pow => OpCode::Pow,
                     BinaryOp::BitAnd => OpCode::BitAnd,
                     BinaryOp::BitOr => OpCode::BitOr,
                     BinaryOp::BitXor => OpCode::BitXor,
                     BinaryOp::Shl => OpCode::Shl,
                     BinaryOp::Shr => OpCode::Shr,
                     BinaryOp::UShr => OpCode::Ushr,
                     BinaryOp::Instanceof => OpCode::Instanceof,
                     BinaryOp::In => OpCode::In,
                 },
                 Some(varn_core::NumericKind::Float) => match op {
                     BinaryOp::Add => OpCode::AddFloat,
                     BinaryOp::Sub => OpCode::SubFloat,
                     BinaryOp::Mul => OpCode::MulFloat,
                     BinaryOp::Div => OpCode::DivFloat,
                     BinaryOp::Eq => OpCode::EqFloat,
                     BinaryOp::NotEq => OpCode::NeqFloat,
                     BinaryOp::Lt => OpCode::LtFloat,
                     BinaryOp::LtEq => OpCode::LteFloat,
                     BinaryOp::Gt => OpCode::GtFloat,
                     BinaryOp::GtEq => OpCode::GteFloat,
                     BinaryOp::Mod => OpCode::Mod,
                     BinaryOp::Pow => OpCode::Pow,
                     BinaryOp::BitAnd => OpCode::BitAnd,
                     BinaryOp::BitOr => OpCode::BitOr,
                     BinaryOp::BitXor => OpCode::BitXor,
                     BinaryOp::Shl => OpCode::Shl,
                     BinaryOp::Shr => OpCode::Shr,
                     BinaryOp::UShr => OpCode::Ushr,
                     BinaryOp::Instanceof => OpCode::Instanceof,
                     BinaryOp::In => OpCode::In,
                 },
                 _ => match op {
                     BinaryOp::Add => OpCode::Add,
                     BinaryOp::Sub => OpCode::Sub,
                     BinaryOp::Mul => OpCode::Mul,
                     BinaryOp::Div => OpCode::Div,
                     BinaryOp::Mod => OpCode::Mod,
                     BinaryOp::Pow => OpCode::Pow,
                     BinaryOp::Eq => OpCode::Eq,
                     BinaryOp::NotEq => OpCode::Neq,
                     BinaryOp::Lt => OpCode::Lt,
                     BinaryOp::LtEq => OpCode::Lte,
                     BinaryOp::Gt => OpCode::Gt,
                     BinaryOp::GtEq => OpCode::Gte,
                     BinaryOp::BitAnd => OpCode::BitAnd,
                     BinaryOp::BitOr => OpCode::BitOr,
                     BinaryOp::BitXor => OpCode::BitXor,
                     BinaryOp::Shl => OpCode::Shl,
                     BinaryOp::Shr => OpCode::Shr,
                     BinaryOp::UShr => OpCode::Ushr,
                     BinaryOp::Instanceof => OpCode::Instanceof,
                     BinaryOp::In => OpCode::In,
                 }
             };

            c.emit_rrr(opcode, l, l, r);
            c.free_reg();

            if let (Some(vreg_l), Some(vreg_r)) = (c.get_vreg_for_phys(l), c.get_vreg_for_phys(r)) {
                let vreg_dest = c.emit_ir_binary(opcode, vreg_l, vreg_r);
                c.map_phys_to_vreg(l, vreg_dest);
            }

            l
        }

        ExprKind::Logical { op, left, right } => {
            let dest = c.alloc_reg();
            let l = compile_expr(c, left);
            c.emit_rr(OpCode::Move, dest, l);
            c.free_reg();
            match op {
                LogicalOp::And => {
                    let skip = c.emit_cond_jump(OpCode::JumpIfFalse, dest);
                    let r = compile_expr(c, right);
                    c.emit_rr(OpCode::Move, dest, r);
                    c.free_reg();
                    c.patch_jump(skip);
                }
                LogicalOp::Or => {
                    let skip = c.emit_cond_jump(OpCode::JumpIfTrue, dest);
                    let r = compile_expr(c, right);
                    c.emit_rr(OpCode::Move, dest, r);
                    c.free_reg();
                    c.patch_jump(skip);
                }
                LogicalOp::Nullish => {
                    let is_null = c.alloc_reg();
                    c.emit_rr(OpCode::IsNull, is_null, dest);
                    let not_null = c.emit_cond_jump(OpCode::JumpIfFalse, is_null);
                    c.free_reg();
                    let r = compile_expr(c, right);
                    c.emit_rr(OpCode::Move, dest, r);
                    c.free_reg();
                    c.patch_jump(not_null);
                }
            }
            dest
        }

        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            let cond = compile_expr(c, test);
            let dest = c.alloc_reg();
            let else_j = c.emit_cond_jump(OpCode::JumpIfFalse, cond);
            c.free_reg();
            let cons = compile_expr(c, consequent);
            c.emit_rr(OpCode::Move, dest, cons);
            c.free_reg();
            let end_j = c.emit_jump(OpCode::Jump);
            c.patch_jump(else_j);
            let alt = compile_expr(c, alternate);
            c.emit_rr(OpCode::Move, dest, alt);
            c.free_reg();
            c.patch_jump(end_j);
            dest
        }

        ExprKind::Assign { op, target, value } => compile_assign(c, op, target, value),

        ExprKind::Member {
            object,
            property,
            computed,
            optional,
        } => compile_member_access(
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
        } => compile_call(c, callee, args, *optional, expr.range.start.offset),

        ExprKind::New { callee, args, .. } => {
            let callee_reg = compile_expr(c, callee);
            let line = c.line;
            let recv_reg = c.alloc_reg();
            c.chunk.emit_rr(OpCode::LoadNull, recv_reg, 0, line);

            let (arg_start, arg_count, has_spread) =
                compile_args_contiguous(c, expr.range.start.offset, args);
            assert_eq!(arg_start, recv_reg + 1, "contiguous args must start right after receiver register");

            let dest = callee_reg;
            if has_spread {
                c.chunk.emit(OpCode::CallSpread, line);
                c.chunk.write(Chunk::pack(dest, callee_reg), line);
                c.chunk.write(Chunk::pack((arg_count + 1) as u8, recv_reg), line);
            } else {
                c.chunk.emit(OpCode::Call, line);
                c.chunk.write(Chunk::pack(dest, callee_reg), line);
                c.chunk.write(
                    Chunk::pack((arg_count + 1) as u8, recv_reg),
                    line,
                );
            }
            for _ in 0..arg_count {
                c.free_reg();
            }
            c.free_reg(); // Free receiver register
            dest
        }

        ExprKind::Array { elements, .. } => compile_array(c, elements),

        ExprKind::Object { properties, .. } => compile_object(c, properties),

        ExprKind::Template { parts } => compile_template(c, parts),

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
            emit_closure(c, proto, upvalues)
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
            emit_closure(c, proto, upvalues)
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

        ExprKind::Pipeline { left, right } => compile_pipeline(c, left, right),

        ExprKind::Match { subject, cases } => compile_match(c, subject, cases),

        ExprKind::Is {
            expression,
            type_ann,
        } => {
            use varn_core::{IntrinsicType, TypeKind, TypeTag};
            let src = compile_expr(c, expression);
            let dest = c.alloc_reg();
            match &type_ann.kind {
                TypeKind::Intrinsic(TypeTag::Null) => {
                    c.emit_rr(OpCode::IsNull, dest, src);
                }
                TypeKind::Intrinsic(TypeTag::Array) | TypeKind::Array(_) => {
                    c.emit_rr(OpCode::IsArray, dest, src);
                }
                TypeKind::Generic(n, _, _) if n == IntrinsicType::Array.as_str() => {
                    c.emit_rr(OpCode::IsArray, dest, src);
                }
                TypeKind::Intrinsic(tt) => {
                    let type_str = c.alloc_reg();
                    c.emit_rr(OpCode::Typeof, type_str, src);
                    let it = IntrinsicType::from(*tt).as_str();
                    let s_idx = c.add_str(it);
                    let s_reg = c.alloc_reg();
                    c.emit_rc(OpCode::LoadConst, s_reg, s_idx);
                    c.emit_rrr(OpCode::Eq, dest, type_str, s_reg);
                    c.free_reg();
                    c.free_reg();
                }
                TypeKind::Named(name, _) => {
                    if let Some(it) = IntrinsicType::from_str(name) {
                        if it.is_scalar_primitive() {
                            let type_str = c.alloc_reg();
                            c.emit_rr(OpCode::Typeof, type_str, src);
                            let s_idx = c.add_str(it.as_str());
                            let s_reg = c.alloc_reg();
                            c.emit_rc(OpCode::LoadConst, s_reg, s_idx);
                            c.emit_rrr(OpCode::Eq, dest, type_str, s_reg);
                            c.free_reg();
                            c.free_reg();
                        } else {
                            let cls = c.alloc_reg();
                            let idx = c.add_str(name);
                            c.emit_rc(OpCode::LoadGlobal, cls, idx);
                            c.emit_rrr(OpCode::Instanceof, dest, src, cls);
                            c.free_reg();
                        }
                    } else {
                        let cls = c.alloc_reg();
                        let idx = c.add_str(name);
                        c.emit_rc(OpCode::LoadGlobal, cls, idx);
                        c.emit_rrr(OpCode::Instanceof, dest, src, cls);
                        c.free_reg();
                    }
                }
                _ => {
                    c.emit_rr(OpCode::LoadFalse, dest, 0);
                }
            }
            c.free_reg();
            dest
        }
    }
}

fn compile_update<'a>(c: &mut Compiler<'a>, op: &UpdateOp, prefix: bool, operand: &Expr) -> u8 {
    match &operand.kind {
        ExprKind::Identifier { name } => {
            let cur = c.alloc_reg();
            if !c.emit_load_var(name, cur) {
                let idx = c.add_str(name);
                c.emit_rc(OpCode::LoadGlobal, cur, idx);
            }
            let one = c.alloc_reg();
            c.emit_load_int(one, 1);
            let next = c.alloc_reg();
            match op {
                UpdateOp::Increment => c.emit_rrr(OpCode::Add, next, cur, one),
                UpdateOp::Decrement => c.emit_rrr(OpCode::Sub, next, cur, one),
            }
            c.free_reg();

            if !c.emit_store_var(name, next) {
                let idx = c.add_str(name);
                let line = c.line;
                c.chunk.emit_rrc(OpCode::StoreGlobal, 0, next, idx, line);
            }
            if prefix {
                c.free_reg();
                next
            } else {
                c.free_reg();
                cur
            }
        }
        _ => compile_expr(c, operand),
    }
}

fn compile_assign<'a>(c: &mut Compiler<'a>, op: &AssignOp, target: &Expr, value: &Expr) -> u8 {
    match op {
        AssignOp::Assign => {
            let val = compile_expr(c, value);
            store_to_target(c, target, val);
            val
        }
        AssignOp::AndAssign => {
            let dest = c.alloc_reg();
            let cur = load_from_target(c, target);
            c.emit_rr(OpCode::Move, dest, cur);
            c.free_reg();
            let skip = c.emit_cond_jump(OpCode::JumpIfFalse, dest);
            let val = compile_expr(c, value);
            c.emit_rr(OpCode::Move, dest, val);
            c.free_reg();
            store_to_target(c, target, dest);
            c.patch_jump(skip);
            dest
        }
        AssignOp::OrAssign => {
            let dest = c.alloc_reg();
            let cur = load_from_target(c, target);
            c.emit_rr(OpCode::Move, dest, cur);
            c.free_reg();
            let skip = c.emit_cond_jump(OpCode::JumpIfTrue, dest);
            let val = compile_expr(c, value);
            c.emit_rr(OpCode::Move, dest, val);
            c.free_reg();
            store_to_target(c, target, dest);
            c.patch_jump(skip);
            dest
        }
        AssignOp::NullishAssign => {
            let dest = c.alloc_reg();
            let cur = load_from_target(c, target);
            c.emit_rr(OpCode::Move, dest, cur);
            c.free_reg();
            let is_null = c.alloc_reg();
            c.emit_rr(OpCode::IsNull, is_null, dest);
            let not_null = c.emit_cond_jump(OpCode::JumpIfFalse, is_null);
            c.free_reg();
            let val = compile_expr(c, value);
            c.emit_rr(OpCode::Move, dest, val);
            c.free_reg();
            store_to_target(c, target, dest);
            c.patch_jump(not_null);
            dest
        }
        _ => {
            let cur = load_from_target(c, target);
            let val = compile_expr(c, value);
            let dest = c.alloc_reg();
            let opcode = compound_to_arith(*op);
            c.emit_rrr(opcode, dest, cur, val);
            c.free_reg();
            c.free_reg();
            store_to_target(c, target, dest);
            dest
        }
    }
}

fn load_from_target<'a>(c: &mut Compiler<'a>, target: &Expr) -> u8 {
    compile_expr(c, target)
}

fn store_to_target<'a>(c: &mut Compiler<'a>, target: &Expr, val: u8) {
    match &target.kind {
        ExprKind::Identifier { name } => {
            if !c.emit_store_var(name, val) {
                let idx = c.add_str(name);
                let line = c.line;
                c.chunk.emit_rrc(OpCode::StoreGlobal, 0, val, idx, line);
            }
        }
        ExprKind::Member {
            object,
            property,
            computed,
            ..
        } => {
            let offset = target.range.start.offset;
            if !computed {
                if let Some(mangled) = c.extension_set_members.get(&offset).cloned() {
                    let setter = c.alloc_reg();
                    let idx = c.add_str(&mangled);
                    c.emit_rc(OpCode::LoadGlobal, setter, idx);

                    let arg0 = c.alloc_reg();
                    let this_val = compile_expr(c, object);
                    c.emit_rr(OpCode::Move, arg0, this_val);
                    c.free_reg();

                    let arg1 = c.alloc_reg();
                    c.emit_rr(OpCode::Move, arg1, val);
                    let line = c.line;
                    c.chunk.emit(OpCode::Call, line);
                    c.chunk.write(Chunk::pack(setter, setter), line);
                    c.chunk.write(Chunk::pack(2, arg0), line);
                    c.free_reg();
                    c.free_reg();
                    c.free_reg();
                    return;
                }
            }
            let obj = compile_expr(c, object);
            if *computed {
                let key = compile_expr(c, property);
                c.emit_rrr(OpCode::SetIndex, obj, key, val);
                c.free_reg();
            } else {
                let name = match &property.as_ref().kind {
                    ExprKind::Identifier { name } => name.clone(),
                    _ => return,
                };
                let idx = c.add_str(&name);
                c.emit_property(OpCode::SetProperty, obj, val, idx);
            }
            c.free_reg();
        }
        _ => {}
    }
}

fn compound_to_arith(op: AssignOp) -> OpCode {
    match op {
        AssignOp::AddAssign => OpCode::Add,
        AssignOp::SubAssign => OpCode::Sub,
        AssignOp::MulAssign => OpCode::Mul,
        AssignOp::DivAssign => OpCode::Div,
        AssignOp::ModAssign => OpCode::Mod,
        AssignOp::PowAssign => OpCode::Pow,
        AssignOp::BitAndAssign => OpCode::BitAnd,
        AssignOp::BitOrAssign => OpCode::BitOr,
        AssignOp::BitXorAssign => OpCode::BitXor,
        AssignOp::ShlAssign => OpCode::Shl,
        AssignOp::ShrAssign => OpCode::Shr,
        AssignOp::UShrAssign => OpCode::Ushr,
        _ => OpCode::Add,
    }
}

fn compile_member_access<'a>(
    c: &mut Compiler<'a>,
    object: &Expr,
    property: &Expr,
    computed: bool,
    optional: bool,
    offset: u32,
) -> u8 {
    if !computed {
        if let Some(mangled) = c.extension_members.get(&offset).cloned() {
            let obj = compile_expr(c, object);
            if optional {
                let is_null = c.alloc_reg();
                c.emit_rr(OpCode::IsNull, is_null, obj);
                let end = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
                c.free_reg();
                let setter = c.alloc_reg();
                let idx = c.add_str(&mangled);
                c.emit_rc(OpCode::LoadGlobal, setter, idx);
                let dest = c.alloc_reg();
                let line = c.line;
                c.chunk.emit(OpCode::Call, line);
                c.chunk.write(Chunk::pack(dest, setter), line);
                c.chunk.write(Chunk::pack(1, obj), line);
                c.free_reg();
                c.free_reg();
                c.patch_jump(end);
                return dest;
            } else {
                let setter = c.alloc_reg();
                let idx = c.add_str(&mangled);
                c.emit_rc(OpCode::LoadGlobal, setter, idx);
                let dest = c.alloc_reg();
                let line = c.line;
                c.chunk.emit(OpCode::Call, line);
                c.chunk.write(Chunk::pack(dest, setter), line);
                c.chunk.write(Chunk::pack(1, obj), line);
                c.free_reg();
                c.free_reg();
                return dest;
            }
        }
    }

    let obj = compile_expr(c, object);

    if optional {
        let is_null = c.alloc_reg();
        c.emit_rr(OpCode::IsNull, is_null, obj);
        let end = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
        c.free_reg();
        if computed {
            let key = compile_expr(c, property);
            c.emit_rrr(OpCode::GetIndex, obj, obj, key);
            c.free_reg();
        } else {
            let name = match &property.kind {
                ExprKind::Identifier { name } => name.clone(),
                _ => {
                    return obj;
                }
            };
            let idx = c.add_str(&name);
            c.emit_rrc(OpCode::GetPropertyMaybe, obj, obj, idx);
        }
        c.patch_jump(end);
        obj
    } else if computed {
        let key = compile_expr(c, property);
        c.emit_rrr(OpCode::GetIndex, obj, obj, key);
        c.free_reg();
        obj
    } else {
        let name = match &property.kind {
            ExprKind::Identifier { name } => name.clone(),
            _ => {
                return obj;
            }
        };
        let idx = c.add_str(&name);

        c.emit_property(OpCode::GetProperty, obj, obj, idx);
        obj
    }
}

fn compile_call<'a>(
    c: &mut Compiler<'a>,
    callee: &Expr,
    args: &[Arg],
    optional: bool,
    offset: u32,
) -> u8 {
    if let Some(mangled) = c.extension_calls.get(&offset).cloned() {
        if let ExprKind::Member {
            object,
            optional: mem_opt,
            ..
        } = &callee.kind
        {
            let obj = compile_expr(c, object);
            if optional || *mem_opt {
                let is_null = c.alloc_reg();
                c.emit_rr(OpCode::IsNull, is_null, obj);
                let end = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
                c.free_reg();
                let dest = emit_extension_call(c, &mangled, obj, offset, args);
                c.free_reg();
                c.patch_jump(end);
                return dest;
            } else {
                let dest = emit_extension_call(c, &mangled, obj, offset, args);
                c.free_reg();
                return dest;
            }
        }
    }

    if optional {
        let callee_reg = compile_expr(c, callee);
        let is_null = c.alloc_reg();
        c.emit_rr(OpCode::IsNull, is_null, callee_reg);
        let end = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
        c.free_reg();
        let dest = emit_plain_call(c, callee_reg, offset, args);

        c.patch_jump(end);
        return dest;
    }

    if let ExprKind::Member {
        object,
        property,
        computed,
        optional: mem_opt,
    } = &callee.kind
    {
        if !computed {
            if let ExprKind::Identifier { name } = &property.as_ref().kind {
                if matches!(&object.as_ref().kind, ExprKind::Super) {
                    let super_idx = c.add_str(name);
                    let fn_reg = c.alloc_reg();
                    c.emit_rc(OpCode::GetSuper, fn_reg, super_idx);
                    let (arg_start, arg_count, has_spread) =
                        compile_args_contiguous(c, offset, args);

                    let dest = fn_reg;
                    let line = c.line;
                    if has_spread {
                        c.chunk.emit(OpCode::CallSpread, line);
                        c.chunk.write(Chunk::pack(dest, fn_reg), line);
                        c.chunk.write(Chunk::pack(arg_count as u8, arg_start), line);
                    } else {
                        c.chunk.emit(OpCode::Call, line);
                        c.chunk.write(Chunk::pack(dest, fn_reg), line);
                        c.chunk.write(
                            Chunk::pack(arg_count as u8, if arg_count > 0 { arg_start } else { 0 }),
                            line,
                        );
                    }
                    for _ in 0..arg_count {
                        c.free_reg();
                    }
                    if name.as_ref() == "constructor" {
                        emit_field_inits(c);
                    }
                    return dest;
                }

                if *mem_opt {
                    let obj = compile_expr(c, object);
                    let is_null = c.alloc_reg();
                    c.emit_rr(OpCode::IsNull, is_null, obj);
                    let end = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
                    c.free_reg();
                    let dest = emit_method_call(c, obj, name, offset, args);

                    c.patch_jump(end);
                    return dest;
                }

                let obj = compile_expr(c, object);
                let dest = emit_method_call(c, obj, name, offset, args);

                return dest;
            }
        }
    }

    if let ExprKind::Super = &callee.kind {
        let super_idx = c.add_str("constructor");
        let fn_reg = c.alloc_reg();
        c.emit_rc(OpCode::GetSuper, fn_reg, super_idx);
        let line = c.line;
        let recv_reg = c.alloc_reg();
        c.emit_rr(OpCode::Move, recv_reg, 0); // "this" is always Register 0

        let (arg_start, arg_count, has_spread) = compile_args_contiguous(c, offset, args);
        assert_eq!(arg_start, recv_reg + 1, "contiguous args must start right after receiver register");

        let dest = fn_reg;
        if has_spread {
            c.chunk.emit(OpCode::CallSpread, line);
            c.chunk.write(Chunk::pack(dest, fn_reg), line);
            c.chunk.write(Chunk::pack((arg_count + 1) as u8, recv_reg), line);
        } else {
            c.chunk.emit(OpCode::Call, line);
            c.chunk.write(Chunk::pack(dest, fn_reg), line);
            c.chunk.write(
                Chunk::pack((arg_count + 1) as u8, recv_reg),
                line,
            );
        }
        for _ in 0..arg_count {
            c.free_reg();
        }
        c.free_reg(); // Free receiver register copy
        if &*c.name == "constructor" {
            emit_field_inits(c);
        }
        return dest;
    }

    let callee_reg = compile_expr(c, callee);

    emit_plain_call(c, callee_reg, offset, args)
}

fn emit_method_call<'a>(
    c: &mut Compiler<'a>,
    obj: u8,
    name: &str,
    offset: u32,
    args: &[Arg],
) -> u8 {
    let (arg_start, arg_count, has_spread) = compile_args_contiguous(c, offset, args);

    let dest = obj;
    let str_idx = c.add_str(name);
    let cs = c.alloc_cache() as u8;
    let line = c.line;
    if has_spread {
        let fn_reg = c.alloc_reg();
        c.emit_property(OpCode::GetProperty, fn_reg, obj, str_idx);
        c.chunk.emit(OpCode::CallSpread, line);
        c.chunk.write(Chunk::pack(dest, fn_reg), line);
        c.chunk.write(Chunk::pack(arg_count as u8, arg_start), line);
        c.free_reg();
    } else {
        c.chunk.write(Chunk::pack_op(OpCode::CallMethod, cs), line);
        c.chunk.write(Chunk::pack(dest, obj), line);
        c.chunk.write(str_idx, line);
        c.chunk.write(Chunk::pack(arg_count as u8, arg_start), line);
    }
    for _ in 0..arg_count {
        c.free_reg();
    }
    dest
}

fn emit_plain_call<'a>(c: &mut Compiler<'a>, callee_reg: u8, offset: u32, args: &[Arg]) -> u8 {
    let line = c.line;
    let recv_reg = c.alloc_reg();
    c.chunk.emit_rr(OpCode::LoadNull, recv_reg, 0, line);

    let (arg_start, arg_count, has_spread) = compile_args_contiguous(c, offset, args);
    assert_eq!(arg_start, recv_reg + 1, "contiguous args must start right after receiver register");

    let dest = callee_reg;
    if has_spread {
        c.chunk.emit(OpCode::CallSpread, line);
        c.chunk.write(Chunk::pack(dest, callee_reg), line);
        c.chunk.write(Chunk::pack((arg_count + 1) as u8, recv_reg), line);
    } else {
        c.chunk.emit(OpCode::Call, line);
        c.chunk.write(Chunk::pack(dest, callee_reg), line);
        c.chunk.write(
            Chunk::pack((arg_count + 1) as u8, recv_reg),
            line,
        );
    }
    for _ in 0..arg_count {
        c.free_reg();
    }
    c.free_reg(); // Free receiver register
    dest
}

fn emit_extension_call<'a>(
    c: &mut Compiler<'a>,
    mangled: &str,
    obj: u8,
    offset: u32,
    args: &[Arg],
) -> u8 {
    let fn_reg = c.alloc_reg();
    let idx = c.add_str(mangled);
    c.emit_rc(OpCode::LoadGlobal, fn_reg, idx);

    let obj_copy = c.alloc_reg();
    c.emit_rr(OpCode::Move, obj_copy, obj);
    let (_arg_start, arg_count, _) = compile_args_contiguous(c, offset, args);

    let dest = c.alloc_reg();
    let line = c.line;
    c.chunk.emit(OpCode::Call, line);
    c.chunk.write(Chunk::pack(dest, fn_reg), line);
    c.chunk
        .write(Chunk::pack((arg_count + 1) as u8, obj_copy), line);
    for _ in 0..arg_count {
        c.free_reg();
    }
    c.free_reg();
    c.free_reg();
    dest
}

pub fn compile_args_contiguous<'a>(
    c: &mut Compiler<'a>,
    call_id: u32,
    args: &[Arg],
) -> (u8, usize, bool) {
    let start = c.regs.next;
    let mut count = 0usize;
    let mut has_spread = false;

    let mapping = c.annotations.get_call_mapping(call_id).cloned();

    if let Some(mapping) = mapping {
        for opt_idx in &mapping {
            if let Some(arg_idx) = opt_idx {
                match &args[*arg_idx] {
                    Arg::Positional(e) | Arg::Named { value: e, .. } => {
                        let r = compile_expr(c, e);
                        let slot = start + count as u8;
                        if r != slot {
                            while c.regs.next <= slot {
                                c.regs.alloc();
                            }
                            c.emit_rr(OpCode::Move, slot, r);
                        }
                    }
                    Arg::Spread(e) => {
                        let r = compile_expr(c, e);
                        let slot = start + count as u8;
                        if r != slot {
                            while c.regs.next <= slot {
                                c.regs.alloc();
                            }
                            c.emit_rr(OpCode::WrapSpread, slot, r);
                        } else {
                            c.emit_rr(OpCode::WrapSpread, r, r);
                        }
                        has_spread = true;
                    }
                }
            } else {
                let slot = c.alloc_reg();
                c.emit_rr(OpCode::LoadNull, slot, 0);
            }
            count += 1;
        }
    } else {
        for arg in args {
            match arg {
                Arg::Positional(e) | Arg::Named { value: e, .. } => {
                    let r = compile_expr(c, e);
                    let slot = start + count as u8;
                    if r != slot {
                        while c.regs.next <= slot {
                            c.regs.alloc();
                        }
                        c.emit_rr(OpCode::Move, slot, r);
                    }
                }
                Arg::Spread(e) => {
                    let r = compile_expr(c, e);
                    let slot = start + count as u8;
                    if r != slot {
                        while c.regs.next <= slot {
                            c.regs.alloc();
                        }
                        c.emit_rr(OpCode::WrapSpread, slot, r);
                    } else {
                        c.emit_rr(OpCode::WrapSpread, r, r);
                    }
                    has_spread = true;
                }
            }
            count += 1;
        }
    }

    (start, count, has_spread)
}

fn compile_array<'a>(c: &mut Compiler<'a>, elements: &[ArrayEl]) -> u8 {
    let has_spread = elements.iter().any(|e| matches!(e, ArrayEl::Spread(_)));

    if !has_spread {
        let count = elements.len() as u8;

        let dest = c.alloc_reg();
        let start = c.regs.next;
        for el in elements {
            match el {
                ArrayEl::Hole => {
                    let r = c.alloc_reg();
                    c.emit_rr(OpCode::LoadNull, r, 0);
                    let _ = r;
                }
                ArrayEl::Expr(e) => {
                    let _ = compile_expr(c, e);
                }
                ArrayEl::Spread(_) => unreachable!(),
            };
        }
        let line = c.line;
        c.chunk.emit(OpCode::BuildArray, line);
        c.chunk.write(Chunk::pack(dest, start), line);
        c.chunk.write(Chunk::pack(count, 0), line);

        for _ in 0..count {
            c.free_reg();
        }
        return dest;
    }

    let arr = c.alloc_reg();
    let line = c.line;
    c.chunk.emit(OpCode::BuildArray, line);
    c.chunk.write(Chunk::pack(arr, 0), line);
    c.chunk.write(Chunk::pack(0, 0), line);

    for el in elements {
        match el {
            ArrayEl::Hole => {
                let null_r = c.alloc_reg();
                c.emit_rr(OpCode::LoadNull, null_r, 0);
                c.emit_rr(OpCode::ArrayPush, arr, null_r);
                c.free_reg();
            }
            ArrayEl::Expr(e) => {
                let r = compile_expr(c, e);
                c.emit_rr(OpCode::ArrayPush, arr, r);
                c.free_reg();
            }
            ArrayEl::Spread(e) => {
                let r = compile_expr(c, e);
                c.emit_rr(OpCode::ArrayExtend, arr, r);
                c.free_reg();
            }
        }
    }
    arr
}

fn compile_object<'a>(c: &mut Compiler<'a>, properties: &[ObjectProp]) -> u8 {
    let mut keys: Vec<Rc<str>> = Vec::new();
    let mut is_simple = true;
    for prop in properties {
        match prop {
            ObjectProp::Property { key, .. } => match key {
                PropKey::Identifier(s) | PropKey::Str(s) => keys.push(Rc::from(s.as_str())),
                PropKey::Int(n) => keys.push(Rc::from(n.to_string().as_str())),
                PropKey::Computed(_) => {
                    is_simple = false;
                    break;
                }
            },
            _ => {
                is_simple = false;
                break;
            }
        }
    }

    if is_simple && !keys.is_empty() {
        let shape_idx = c.add_const(PoolEntry::Shape(keys));
        let count = properties.len() as u8;

        let dest = c.alloc_reg();
        let start = c.regs.next;
        for prop in properties {
            if let ObjectProp::Property { value, .. } = prop {
                let _ = compile_expr(c, value);
            }
        }
        let line = c.line;
        c.chunk.emit(OpCode::BuildObjectWithShape, line);
        c.chunk.write(Chunk::pack(dest, start), line);
        c.chunk.write(shape_idx, line);
        for _ in 0..count {
            c.free_reg();
        }
        return dest;
    }

    let dest = c.alloc_reg();
    let mut pending_pairs: Vec<(u16, u8)> = Vec::new();

    let flush_segment = |c: &mut Compiler<'a>, dest: u8, pairs: &mut Vec<(u16, u8)>| {
        let count = pairs.len() as u8;
        let line = c.line;
        c.chunk.emit(OpCode::BuildObject, line);
        c.chunk.write(Chunk::pack(dest, count), line);
        for (key_idx, val_reg) in pairs.iter() {
            c.chunk.write(*key_idx, line);
            c.chunk.write(Chunk::pack(*val_reg as u8, 0), line);
        }
        for _ in pairs.iter() {
            c.free_reg();
        }
        pairs.clear();
    };

    let mut first_segment = true;

    for prop in properties {
        match prop {
            ObjectProp::Property { key, value, .. } => {
                let key_idx = match key {
                    PropKey::Identifier(s) | PropKey::Str(s) => c.add_str(s),
                    PropKey::Int(n) => {
                        let s = n.to_string();
                        c.add_str(&s)
                    }
                    PropKey::Computed(e) => {
                        let r = compile_expr(c, e);
                        let str_r = c.alloc_reg();
                        c.emit_rr(OpCode::ToString, str_r, r);
                        c.free_reg();

                        let s = format!("__computed_{}__", str_r);
                        c.add_str(&s)
                    }
                };
                let val_reg = compile_expr(c, value);
                pending_pairs.push((key_idx, val_reg));
            }
            ObjectProp::Method {
                key,
                params,
                body,
                is_async,
                is_generator,
                ..
            } => {
                let key_str = match key {
                    PropKey::Identifier(s) | PropKey::Str(s) => s.clone(),
                    PropKey::Int(n) => n.to_string(),
                    PropKey::Computed(_) => "<computed>".to_owned(),
                };
                let key_idx = c.add_str(&key_str);
                let (proto, upvalues) = compile_function(
                    c,
                    Rc::from(key_str),
                    params,
                    body,
                    *is_async,
                    *is_generator,
                    true,
                );
                let closure_reg = emit_closure(c, proto, upvalues);
                pending_pairs.push((key_idx, closure_reg));
            }
            ObjectProp::Spread { argument, .. } => {
                if first_segment {
                    flush_segment(c, dest, &mut pending_pairs);
                    first_segment = false;
                } else if !pending_pairs.is_empty() {
                    let tmp = c.alloc_reg();
                    flush_segment(c, tmp, &mut pending_pairs);
                    c.emit_rr(OpCode::ObjectMerge, dest, tmp);
                    c.free_reg();
                }
                let src = compile_expr(c, argument);
                c.emit_rr(OpCode::ObjectMerge, dest, src);
                c.free_reg();
            }
            _ => {}
        }
    }

    if first_segment {
        flush_segment(c, dest, &mut pending_pairs);
    } else if !pending_pairs.is_empty() {
        let tmp = c.alloc_reg();
        flush_segment(c, tmp, &mut pending_pairs);
        c.emit_rr(OpCode::ObjectMerge, dest, tmp);
        c.free_reg();
    }

    dest
}

fn compile_template<'a>(c: &mut Compiler<'a>, parts: &[TemplatePart]) -> u8 {
    if parts.is_empty() {
        let r = c.alloc_reg();
        let idx = c.add_str("");
        c.emit_rc(OpCode::LoadConst, r, idx);
        return r;
    }

    // Single literal: no concat needed.
    if parts.len() == 1 {
        if let TemplatePart::Literal(s) = &parts[0] {
            let r = c.alloc_reg();
            let idx = c.add_str(s);
            c.emit_rc(OpCode::LoadConst, r, idx);
            return r;
        }
    }

    // Compile each part into a register (literals as LoadConst, expressions as ToString).
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
    c.chunk.write(crate::chunk::Chunk::pack_op(OpCode::BuildStr, dest), line);
    c.chunk.write(crate::chunk::Chunk::pack(count, 0), line);
    for &reg in &part_regs {
        c.chunk.write(crate::chunk::Chunk::pack(reg, 0), line);
    }

    // Free all part registers in reverse LIFO order.
    for _ in &part_regs {
        c.free_reg();
    }

    dest
}

fn compile_pipeline<'a>(c: &mut Compiler<'a>, left: &Expr, right: &Expr) -> u8 {
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
        emit_closure(c, proto, upvalues)
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
    c.free_reg(); // Free arg register
    c.free_reg(); // Free receiver register
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

fn compile_match<'a>(
    c: &mut Compiler<'a>,
    subject: &Expr,
    cases: &[varn_core::ast::expr::MatchCase],
) -> u8 {
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
                let tag_key = c.add_str(MemberKey::Tag.as_str());
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
                super::stmt::compile_stmt(c, s);
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

pub fn emit_field_inits<'a>(c: &mut Compiler<'a>) {
    if c.pending_field_inits.is_empty() {
        return;
    }
    let inits = std::mem::take(&mut c.pending_field_inits);
    for (name, expr) in inits {
        let this_r = 0u8;
        let val = compile_expr(c, &expr);
        let key_idx = c.add_str(&name);
        c.emit_property(OpCode::SetProperty, this_r, val, key_idx);
        c.free_reg();
    }
}

/// Emit AddImm/SubImm when one operand is a small integer constant (-128..=127).
/// Saves one instruction vs LoadInt + Add/Sub.
fn try_emit_imm_op<'a>(
    c: &mut Compiler<'a>,
    op: &BinaryOp,
    left: &Expr,
    right: &Expr,
) -> Option<u8> {
    let (src_expr, imm, is_sub) = match op {
        BinaryOp::Add => {
            if let Some(k) = small_const_int(right) {
                (left, k, false)
            } else if let Some(k) = small_const_int(left) {
                (right, k, false)
            } else {
                return None;
            }
        }
        BinaryOp::Sub => {
            if let Some(k) = small_const_int(right) {
                (left, k, true)
            } else {
                return None;
            }
        }
        _ => return None,
    };
    // Compile src, write result back into src's register (matches Add pattern).
    let src = compile_expr(c, src_expr);
    let line = c.line;
    let opcode = if is_sub { OpCode::SubImm } else { OpCode::AddImm };
    let imm_byte = imm as i8 as u8;
    c.chunk.write(crate::chunk::Chunk::pack_op(opcode, src), line);
    c.chunk.write(crate::chunk::Chunk::pack(src, imm_byte), line);
    Some(src)
}

fn small_const_int(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::IntLiteral { value, .. } if *value >= -128 && *value <= 127 => Some(*value),
        ExprKind::Unary { op: UnaryOp::Minus, operand, .. } => {
            if let ExprKind::IntLiteral { value, .. } = &operand.kind {
                let neg = value.checked_neg()?;
                if neg >= -128 && neg <= 127 { Some(neg) } else { None }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Try to evaluate a binary expression at compile time.
/// Returns `Some(reg)` if folded, `None` if runtime computation needed.
fn try_fold_binary<'a>(
    c: &mut Compiler<'a>,
    op: &BinaryOp,
    left: &Expr,
    right: &Expr,
) -> Option<u8> {
    use BinaryOp::*;
    let lv = const_int_value(left)?;
    let rv = const_int_value(right)?;

    // Integer arithmetic folding
    let arith: Option<i64> = match op {
        Add => lv.checked_add(rv),
        Sub => lv.checked_sub(rv),
        Mul => lv.checked_mul(rv),
        Div if rv != 0 && lv % rv == 0 => Some(lv / rv),
        Mod if rv != 0 => Some(lv % rv),
        Shl => lv.checked_shl(rv as u32),
        Shr => Some(lv >> (rv as u32).min(63)),
        BitAnd => Some(lv & rv),
        BitOr => Some(lv | rv),
        BitXor => Some(lv ^ rv),
        _ => None,
    };
    if let Some(result) = arith {
        let r = c.alloc_reg();
        let line = c.line;
        c.chunk.emit_load_int(r, result, line);
        return Some(r);
    }

    // Boolean comparison folding
    let bool_result: bool = match op {
        Eq => lv == rv,
        NotEq => lv != rv,
        Lt => lv < rv,
        LtEq => lv <= rv,
        Gt => lv > rv,
        GtEq => lv >= rv,
        _ => return None,
    };
    let r = c.alloc_reg();
    let line = c.line;
    let bool_op = if bool_result { OpCode::LoadTrue } else { OpCode::LoadFalse };
    c.chunk.emit_rr(bool_op, r, 0, line);
    Some(r)
}

fn const_int_value(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::IntLiteral { value, .. } => Some(*value),
        ExprKind::Unary {
            op: UnaryOp::Minus,
            operand,
            ..
        } => const_int_value(operand).and_then(|v| v.checked_neg()),
        _ => None,
    }
}
