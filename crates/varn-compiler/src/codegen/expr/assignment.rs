use super::super::compiler::Compiler;
use crate::chunk::Chunk;
use varn_core::ast::operators::{AssignOp, UpdateOp};
use varn_core::ast::{Expr, ExprKind};
use varn_core::OpCode;

use super::compile_expr;

pub(super) fn compile_update<'a>(
    c: &mut Compiler<'a>,
    op: &UpdateOp,
    prefix: bool,
    operand: &Expr,
) -> u8 {
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
        ExprKind::Member {
            object,
            property,
            computed,
            ..
        } => {
            let obj = compile_expr(c, object);
            let cur = c.alloc_reg();
            if *computed {
                let key = compile_expr(c, property);
                c.emit_rrr(OpCode::GetIndex, cur, obj, key);

                let one = c.alloc_reg();
                c.emit_load_int(one, 1);
                let next = c.alloc_reg();
                match op {
                    UpdateOp::Increment => c.emit_rrr(OpCode::Add, next, cur, one),
                    UpdateOp::Decrement => c.emit_rrr(OpCode::Sub, next, cur, one),
                }
                c.free_reg(); // free one

                c.emit_rrr(OpCode::SetIndex, obj, key, next);
                c.free_reg(); // free key
                c.free_reg(); // free obj

                if prefix {
                    c.free_reg(); // free cur
                    next
                } else {
                    c.free_reg(); // free next
                    cur
                }
            } else {
                let name = match &property.as_ref().kind {
                    ExprKind::Identifier { name } => name.clone(),
                    _ => return compile_expr(c, operand),
                };
                let idx = c.add_str(&name);
                c.emit_property(OpCode::GetProperty, cur, obj, idx);

                let one = c.alloc_reg();
                c.emit_load_int(one, 1);
                let next = c.alloc_reg();
                match op {
                    UpdateOp::Increment => c.emit_rrr(OpCode::Add, next, cur, one),
                    UpdateOp::Decrement => c.emit_rrr(OpCode::Sub, next, cur, one),
                }
                c.free_reg(); // free one

                c.emit_property(OpCode::SetProperty, obj, next, idx);
                c.free_reg(); // free obj

                if prefix {
                    c.free_reg(); // free cur
                    next
                } else {
                    c.free_reg(); // free next
                    cur
                }
            }
        }
        _ => compile_expr(c, operand),
    }
}

pub(super) fn compile_assign<'a>(
    c: &mut Compiler<'a>,
    op: &AssignOp,
    target: &Expr,
    value: &Expr,
) -> u8 {
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
