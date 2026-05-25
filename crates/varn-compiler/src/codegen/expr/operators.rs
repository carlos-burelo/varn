use super::super::compiler::Compiler;
use crate::chunk::{Literal, PoolEntry};
use varn_core::ast::operators::{BinaryOp, LogicalOp, UnaryOp};
use varn_core::ast::{Expr, ExprKind};
use varn_core::OpCode;

use super::compile_expr;

pub(super) fn compile_try_expr<'a>(c: &mut Compiler<'a>, expression: &Expr) -> u8 {
    use crate::chunk::Chunk;
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

pub(super) fn compile_conditional<'a>(
    c: &mut Compiler<'a>,
    test: &Expr,
    consequent: &Expr,
    alternate: &Expr,
) -> u8 {
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

pub(super) fn compile_logical<'a>(
    c: &mut Compiler<'a>,
    op: &LogicalOp,
    left: &Expr,
    right: &Expr,
) -> u8 {
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

pub(super) fn compile_unary<'a>(c: &mut Compiler<'a>, op: &UnaryOp, operand: &Expr) -> u8 {
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

pub(super) fn compile_binary<'a>(
    c: &mut Compiler<'a>,
    op: &BinaryOp,
    left: &Expr,
    right: &Expr,
    expr_offset: u32,
) -> u8 {
    if let Some(folded) = try_fold_binary(c, op, left, right) {
        return folded;
    }
    if let Some(r) = try_emit_imm_op(c, op, left, right) {
        return r;
    }
    let l = compile_expr(c, left);
    let r = compile_expr(c, right);
    let numeric_kind = c.annotations.get_numeric(expr_offset);
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
        },
    };

    c.emit_rrr(opcode, l, l, r);
    c.free_reg();

    if let (Some(vreg_l), Some(vreg_r)) = (c.get_vreg_for_phys(l), c.get_vreg_for_phys(r)) {
        let vreg_dest = c.emit_ir_binary(opcode, vreg_l, vreg_r);
        c.map_phys_to_vreg(l, vreg_dest);
    }

    l
}

pub(super) fn try_emit_imm_op<'a>(
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
    let src = compile_expr(c, src_expr);
    let line = c.line;
    let opcode = if is_sub {
        OpCode::SubImm
    } else {
        OpCode::AddImm
    };
    let imm_byte = imm as i8 as u8;
    c.chunk
        .write(crate::chunk::Chunk::pack_op(opcode, src), line);
    c.chunk
        .write(crate::chunk::Chunk::pack(src, imm_byte), line);
    Some(src)
}

pub(super) fn small_const_int(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::IntLiteral { value, .. } if *value >= -128 && *value <= 127 => Some(*value),
        ExprKind::Unary {
            op: UnaryOp::Minus,
            operand,
            ..
        } => {
            if let ExprKind::IntLiteral { value, .. } = &operand.kind {
                let neg = value.checked_neg()?;
                if neg >= -128 && neg <= 127 {
                    Some(neg)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(super) fn try_fold_binary<'a>(
    c: &mut Compiler<'a>,
    op: &BinaryOp,
    left: &Expr,
    right: &Expr,
) -> Option<u8> {
    use BinaryOp::*;
    let lv = const_int_value(left)?;
    let rv = const_int_value(right)?;

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
    let bool_op = if bool_result {
        OpCode::LoadTrue
    } else {
        OpCode::LoadFalse
    };
    c.chunk.emit_rr(bool_op, r, 0, line);
    Some(r)
}

pub(super) fn const_int_value(expr: &Expr) -> Option<i64> {
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
