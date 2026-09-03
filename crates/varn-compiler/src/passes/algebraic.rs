//! Identity simplification: arithmetic whose result is already one of its
//! operands, or a constant, independent of the other operand's value.
//!
//! `const_fold` only fires when *both* operands are known. This pass covers
//! the case where one is: `i + 0`, `x * 1`, `n - n`. Hand-written code rarely
//! contains these, but desugaring and constant propagation produce them, and
//! each one costs a whole instruction plus the register holding the literal.
//!
//! Every rule here has to hold for **all** values of the surviving operand,
//! which is why the float set is so much smaller than the integer one:
//!
//! * `x + 0.0` is not `x` — for `x = -0.0` it yields `+0.0`.
//! * `x * 0.0` is not `0.0` — for `NaN` or an infinity it yields `NaN`.
//! * `x - x` is not `0.0` — again `NaN` and the infinities.
//!
//! Multiplying or dividing a float by one *is* exact for every value
//! including `NaN`, the infinities and both zeros, so those two stay.
//!
//! Rewrites are collected and applied in one traversal; the instructions they
//! orphan are deleted by DCE on the same fixpoint round.

use rustc_hash::FxHashMap;

use crate::hir::{HirBinOp, HirType};
use crate::ssa::ir::{Inst, InstKind, SsaFunc, Value};
use crate::ssa::uses::replace_uses_with_map;

pub fn run(func: &mut SsaFunc) -> bool {
    // Known integer constants, and known float constants, by value id. SSA
    // guarantees a single definition, so one map over the whole function is
    // enough regardless of block order.
    let mut int_const: FxHashMap<Value, i64> = FxHashMap::default();
    let mut float_const: FxHashMap<Value, f64> = FxHashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            match (inst.dest, &inst.kind) {
                (Some(d), InstKind::ConstInt(i)) => {
                    int_const.insert(d, *i);
                }
                (Some(d), InstKind::ConstFloat(f)) => {
                    float_const.insert(d, *f);
                }
                _ => {}
            }
        }
    }

    // `dest -> existing value` rewrites, plus instructions that collapse to a
    // fresh constant and are rewritten in place.
    let mut rewrites: FxHashMap<Value, Value> = FxHashMap::default();
    let mut to_const: Vec<(usize, usize, InstKind)> = Vec::new();

    for (b, block) in func.blocks.iter().enumerate() {
        for (i, inst) in block.insts.iter().enumerate() {
            match simplify(inst, &int_const, &float_const) {
                Some(Simplified::Use(v)) => {
                    // `ty` on a `Binary` is the *result* type, and it need not
                    // match its operands': `int / int` is Float, and so is
                    // `1.0 * n` for an integer `n`. Forwarding the surviving
                    // operand is only valid when it already has the type the
                    // result had, or every consumer downstream — including
                    // `register_meta` and the JIT's unboxing — would be
                    // reading an int where a float was promised.
                    let dest = inst.dest.expect("simplified inst defines a value");
                    if func.value_ty(v) == func.value_ty(dest) {
                        rewrites.insert(dest, v);
                    }
                }
                Some(Simplified::Const(kind)) => to_const.push((b, i, kind)),
                None => {}
            }
        }
    }

    let mut changed = !to_const.is_empty();
    for (b, i, kind) in to_const {
        if let Some(dest) = func.blocks[b].insts[i].dest {
            if let Some(ty) = const_inst_ty(&kind) {
                func.values[dest.0 as usize].ty = ty;
            }
        }
        func.blocks[b].insts[i].kind = kind;
    }
    changed |= replace_uses_with_map(func, &rewrites);
    changed
}

enum Simplified {
    /// The instruction's result is exactly this already-computed value.
    Use(Value),
    /// The instruction's result is this literal, whatever the operands hold.
    Const(InstKind),
}

fn simplify(
    inst: &Inst,
    int_const: &FxHashMap<Value, i64>,
    float_const: &FxHashMap<Value, f64>,
) -> Option<Simplified> {
    let InstKind::Binary { op, lhs, rhs, ty } = &inst.kind else {
        return None;
    };
    let (l, r) = (*lhs, *rhs);

    match ty {
        HirType::Int => {
            let li = int_const.get(&l).copied();
            let ri = int_const.get(&r).copied();
            match op {
                HirBinOp::Add => match (li, ri) {
                    (_, Some(0)) => Some(Simplified::Use(l)),
                    (Some(0), _) => Some(Simplified::Use(r)),
                    _ => None,
                },
                HirBinOp::Sub => {
                    if ri == Some(0) {
                        Some(Simplified::Use(l))
                    } else if l == r {
                        Some(Simplified::Const(InstKind::ConstInt(0)))
                    } else {
                        None
                    }
                }
                HirBinOp::Mul => match (li, ri) {
                    (_, Some(1)) => Some(Simplified::Use(l)),
                    (Some(1), _) => Some(Simplified::Use(r)),
                    // Sound for integers precisely because there is no NaN.
                    (_, Some(0)) | (Some(0), _) => Some(Simplified::Const(InstKind::ConstInt(0))),
                    _ => None,
                },
                // Deliberately nothing for the bitwise and shift operators.
                // They lower as `bor.dyn` and friends — the compiler types
                // them `Dynamic` even when both operands are `int`, and under
                // `Dynamic` `x | 0` is not an identity, since a non-integer
                // operand coerces instead of passing through. They would
                // belong here the day those ops get typed variants.
                //
                // `x ** 1` is left alone too: `Pow` raises on a negative
                // exponent so DCE cannot delete it, and forwarding the result
                // while the instruction still executes saves nothing.
                _ => None,
            }
        }
        HirType::Float => {
            let is_one = |v: Value| float_const.get(&v).copied() == Some(1.0);
            match op {
                HirBinOp::Mul => {
                    if is_one(r) {
                        Some(Simplified::Use(l))
                    } else if is_one(l) {
                        Some(Simplified::Use(r))
                    } else {
                        None
                    }
                }
                HirBinOp::Div => is_one(r).then_some(Simplified::Use(l)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn const_inst_ty(kind: &InstKind) -> Option<HirType> {
    match kind {
        InstKind::ConstInt(_) => Some(HirType::Int),
        InstKind::ConstFloat(_) => Some(HirType::Float),
        InstKind::ConstBool(_) => Some(HirType::Bool),
        InstKind::ConstStr(_) => Some(HirType::Str),
        InstKind::ConstNull => Some(HirType::Dynamic),
        _ => None,
    }
}
