use crate::hir::{HirBinOp, HirType, HirUnOp};
use crate::ssa::ir::{BlockId, InstKind, SsaFunc, Terminator, Value};
use rustc_hash::FxHashMap;
use varn_core::{add_i48, mul_i48, neg_i48, pow_i48, sub_i48, wrap_i48};

pub fn run(func: &mut SsaFunc) -> bool {
    let mut changed = false;
    let mut const_map = FxHashMap::default();

    for block_idx in 0..func.blocks.len() {
        let b_id = BlockId(block_idx as u32);

        for inst_idx in 0..func.blocks[b_id.0 as usize].insts.len() {
            let inst = &func.blocks[b_id.0 as usize].insts[inst_idx];
            if let Some(dest) = inst.dest {
                if is_constant_kind(&inst.kind) {
                    const_map.insert(dest, inst.kind.clone());
                    continue;
                }

                if let Some(folded_kind) = fold_inst(&inst.kind, &const_map) {
                    func.blocks[b_id.0 as usize].insts[inst_idx].kind = folded_kind.clone();
                    const_map.insert(dest, folded_kind);
                    changed = true;
                }
            }
        }

        let term = func.blocks[b_id.0 as usize].term.clone();
        if let Terminator::Branch {
            cond,
            then_blk,
            then_args,
            else_blk,
            else_args,
        } = term
        {
            if let Some(InstKind::ConstBool(b)) = const_map.get(&cond) {
                if *b {
                    func.blocks[b_id.0 as usize].term = Terminator::Jump {
                        target: then_blk,
                        args: then_args,
                    };
                    let preds = &mut func.blocks[else_blk.0 as usize].preds;
                    if let Some(pos) = preds.iter().position(|p| *p == b_id) {
                        preds.remove(pos);
                    }
                } else {
                    func.blocks[b_id.0 as usize].term = Terminator::Jump {
                        target: else_blk,
                        args: else_args,
                    };
                    let preds = &mut func.blocks[then_blk.0 as usize].preds;
                    if let Some(pos) = preds.iter().position(|p| *p == b_id) {
                        preds.remove(pos);
                    }
                }
                changed = true;
            }
        }
    }

    changed
}

fn is_constant_kind(kind: &InstKind) -> bool {
    matches!(
        kind,
        InstKind::ConstInt(_)
            | InstKind::ConstFloat(_)
            | InstKind::ConstBool(_)
            | InstKind::ConstStr(_)
            | InstKind::ConstChar(_)
            | InstKind::ConstDecimal(_)
            | InstKind::ConstBigInt(_)
            | InstKind::ConstNull
    )
}

fn fold_inst(kind: &InstKind, const_map: &FxHashMap<Value, InstKind>) -> Option<InstKind> {
    match kind {
        InstKind::Unary { op, operand, ty } => {
            let operand_const = const_map.get(operand)?;
            fold_unary(*op, operand_const, *ty)
        }
        InstKind::Binary { op, lhs, rhs, ty } => {
            let lhs_const = const_map.get(lhs)?;
            let rhs_const = const_map.get(rhs)?;
            fold_binary(*op, lhs_const, rhs_const, *ty)
        }
        InstKind::IsNull { operand } => {
            let operand_const = const_map.get(operand)?;
            match operand_const {
                InstKind::ConstNull => Some(InstKind::ConstBool(true)),
                _ => Some(InstKind::ConstBool(false)),
            }
        }
        _ => None,
    }
}

fn fold_unary(op: HirUnOp, operand: &InstKind, _ty: HirType) -> Option<InstKind> {
    match (op, operand) {
        // `-INT_MIN` has no i48 form. Declining to fold leaves the negation in
        // the IR, where the interpreter/JIT raise `integer overflow` with a
        // line number — never folding to a wrong constant.
        (HirUnOp::Neg, InstKind::ConstInt(x)) => neg_i48(*x).map(InstKind::ConstInt),
        (HirUnOp::Neg, InstKind::ConstFloat(x)) => Some(InstKind::ConstFloat(-x)),
        (HirUnOp::Not, InstKind::ConstBool(x)) => Some(InstKind::ConstBool(!x)),
        (HirUnOp::BitNot, InstKind::ConstInt(x)) => Some(InstKind::ConstInt(!x)),
        _ => None,
    }
}

fn fold_binary(op: HirBinOp, lhs: &InstKind, rhs: &InstKind, _ty: HirType) -> Option<InstKind> {
    use HirBinOp::*;
    match (lhs, rhs) {
        (InstKind::ConstInt(x), InstKind::ConstInt(y)) => match op {
            // Integer arithmetic that leaves the i48 range RAISES
            // (varn_core::numeric), so an overflowing expression must not be
            // folded to a value: `None` keeps the operation in the IR and the
            // interpreter/JIT raise it at run time, pointing at the line.
            // Folding it — as this did — baked a wrong constant into the pool.
            //
            // A compile-time diagnostic would be better than a run-time one,
            // but it belongs in the checker, which has spans; this pass has
            // neither spans nor an error channel.
            Add => add_i48(*x, *y).map(InstKind::ConstInt),
            Sub => sub_i48(*x, *y).map(InstKind::ConstInt),
            Mul => mul_i48(*x, *y).map(InstKind::ConstInt),
            // `int / int` always yields float.
            Div => {
                if *y != 0 {
                    Some(InstKind::ConstFloat(*x as f64 / *y as f64))
                } else {
                    None
                }
            }
            Mod => {
                if *y != 0 {
                    Some(InstKind::ConstInt(x % y))
                } else {
                    None
                }
            }
            // Negative exponents raise at runtime; never fold them.
            Pow => {
                if *y >= 0 && *y <= 30 {
                    pow_i48(*x, *y as u32).map(InstKind::ConstInt)
                } else {
                    None
                }
            }
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            Lt => Some(InstKind::ConstBool(x < y)),
            Le => Some(InstKind::ConstBool(x <= y)),
            Gt => Some(InstKind::ConstBool(x > y)),
            Ge => Some(InstKind::ConstBool(x >= y)),
            BitAnd => Some(InstKind::ConstInt(x & y)),
            BitOr => Some(InstKind::ConstInt(x | y)),
            BitXor => Some(InstKind::ConstInt(x ^ y)),
            // Shifts truncate to the type width by definition — that is the
            // shift's result, not an overflow — so they keep `wrap_i48`, which
            // is also what `VmValue::from_int` does to the runtime result.
            Shl => Some(InstKind::ConstInt(wrap_i48(x.wrapping_shl(*y as u32)))),
            Shr => Some(InstKind::ConstInt(wrap_i48(x.wrapping_shr(*y as u32)))),
            Ushr => {
                let ux = *x as u64;
                let uy = *y as u32;
                Some(InstKind::ConstInt(wrap_i48((ux >> uy) as i64)))
            }
            _ => None,
        },
        (InstKind::ConstFloat(x), InstKind::ConstFloat(y)) => match op {
            Add => Some(InstKind::ConstFloat(x + y)),
            Sub => Some(InstKind::ConstFloat(x - y)),
            Mul => Some(InstKind::ConstFloat(x * y)),
            Div => Some(InstKind::ConstFloat(x / y)),
            Mod => Some(InstKind::ConstFloat(x % y)),
            Pow => Some(InstKind::ConstFloat(x.powf(*y))),
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            Lt => Some(InstKind::ConstBool(x < y)),
            Le => Some(InstKind::ConstBool(x <= y)),
            Gt => Some(InstKind::ConstBool(x > y)),
            Ge => Some(InstKind::ConstBool(x >= y)),
            _ => None,
        },
        (InstKind::ConstBool(x), InstKind::ConstBool(y)) => match op {
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            _ => None,
        },
        (InstKind::ConstStr(x), InstKind::ConstStr(y)) => match op {
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            _ => None,
        },
        (InstKind::ConstChar(x), InstKind::ConstChar(y)) => match op {
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            _ => None,
        },
        (InstKind::ConstDecimal(x), InstKind::ConstDecimal(y)) => match op {
            Add => x.checked_add(*y).map(InstKind::ConstDecimal),
            Sub => x.checked_sub(*y).map(InstKind::ConstDecimal),
            Mul => x.checked_mul(*y).map(InstKind::ConstDecimal),
            Div => x.checked_div(*y).map(InstKind::ConstDecimal),
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            Lt => Some(InstKind::ConstBool(x < y)),
            Le => Some(InstKind::ConstBool(x <= y)),
            Gt => Some(InstKind::ConstBool(x > y)),
            Ge => Some(InstKind::ConstBool(x >= y)),
            _ => None,
        },
        (InstKind::ConstBigInt(x), InstKind::ConstBigInt(y)) => match op {
            Add => Some(InstKind::ConstBigInt(x.wrapping_add(*y))),
            Sub => Some(InstKind::ConstBigInt(x.wrapping_sub(*y))),
            Mul => Some(InstKind::ConstBigInt(x.wrapping_mul(*y))),
            Div => {
                if *y != 0 {
                    Some(InstKind::ConstBigInt(x / y))
                } else {
                    None
                }
            }
            Mod => {
                if *y != 0 {
                    Some(InstKind::ConstBigInt(x % y))
                } else {
                    None
                }
            }
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            Lt => Some(InstKind::ConstBool(x < y)),
            Le => Some(InstKind::ConstBool(x <= y)),
            Gt => Some(InstKind::ConstBool(x > y)),
            Ge => Some(InstKind::ConstBool(x >= y)),
            _ => None,
        },
        (InstKind::ConstNull, InstKind::ConstNull) => match op {
            Eq => Some(InstKind::ConstBool(true)),
            Ne => Some(InstKind::ConstBool(false)),
            _ => None,
        },
        (InstKind::ConstNull, other) | (other, InstKind::ConstNull) => {
            if is_constant_kind(other) {
                match op {
                    Eq => Some(InstKind::ConstBool(false)),
                    Ne => Some(InstKind::ConstBool(true)),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}
