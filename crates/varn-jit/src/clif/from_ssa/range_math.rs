use rustc_hash::FxHashMap;
use varn_types::ssa::{SsaBinOp, SsaOp, SsaProto, SsaTerm};

pub(super) const I64_LO: i128 = i64::MIN as i128;

pub(super) const I64_HI: i128 = i64::MAX as i128;

pub(super) fn add_range(a: (i128, i128), b: (i128, i128)) -> Option<(i128, i128)> {
    Some((a.0.checked_add(b.0)?, a.1.checked_add(b.1)?))
}

pub(super) fn sub_range(a: (i128, i128), b: (i128, i128)) -> Option<(i128, i128)> {
    Some((a.0.checked_sub(b.1)?, a.1.checked_sub(b.0)?))
}

pub(super) fn mul_range(a: (i128, i128), b: (i128, i128)) -> Option<(i128, i128)> {
    let ps = [
        a.0.checked_mul(b.0)?,
        a.0.checked_mul(b.1)?,
        a.1.checked_mul(b.0)?,
        a.1.checked_mul(b.1)?,
    ];
    Some((*ps.iter().min().unwrap(), *ps.iter().max().unwrap()))
}

pub(super) fn as_const(ranges: &FxHashMap<u32, (i128, i128)>, v: u32) -> Option<i128> {
    match ranges.get(&v) {
        Some((l, h)) if l == h => Some(*l),
        _ => None,
    }
}

pub(super) fn is_one(
    def: &[Option<&SsaOp>],
    ranges: &FxHashMap<u32, (i128, i128)>,
    v: u32,
) -> bool {
    if matches!(def[v as usize], Some(SsaOp::ConstInt(1))) {
        return true;
    }
    as_const(ranges, v) == Some(1)
}

pub(super) struct Check {
    pub(super) bound: i128,
    pub(super) below: bool,
    pub(super) le: bool,
}

pub(super) fn header_check(
    ssa: &SsaProto,
    def: &[Option<&SsaOp>],
    ranges: &FxHashMap<u32, (i128, i128)>,
    header: usize,
    param: u32,
) -> Option<Check> {
    let blk = &ssa.blocks[header];
    let SsaTerm::Branch { cond, .. } = &blk.term else {
        return None;
    };
    let Some(SsaOp::Binary { op, lhs, rhs }) = def[*cond as usize] else {
        return None;
    };
    if !blk.params.contains(&param) {
        return None;
    }
    let le = matches!(op, SsaBinOp::IntLe | SsaBinOp::IntGe);
    match op {
        SsaBinOp::IntLt | SsaBinOp::IntLe => {
            if *lhs == param {
                Some(Check {
                    bound: as_const(ranges, *rhs)?,
                    below: true,
                    le,
                })
            } else if *rhs == param {
                Some(Check {
                    bound: as_const(ranges, *lhs)?,
                    below: false,
                    le,
                })
            } else {
                None
            }
        }
        SsaBinOp::IntGt | SsaBinOp::IntGe => {
            if *lhs == param {
                Some(Check {
                    bound: as_const(ranges, *rhs)?,
                    below: false,
                    le,
                })
            } else if *rhs == param {
                Some(Check {
                    bound: as_const(ranges, *lhs)?,
                    below: true,
                    le,
                })
            } else {
                None
            }
        }
        SsaBinOp::IntAdd
        | SsaBinOp::IntSub
        | SsaBinOp::IntMul
        | SsaBinOp::IntDiv
        | SsaBinOp::IntMod
        | SsaBinOp::IntPow
        | SsaBinOp::IntEq
        | SsaBinOp::IntNe
        | SsaBinOp::IntAnd
        | SsaBinOp::IntOr
        | SsaBinOp::IntXor
        | SsaBinOp::IntShl
        | SsaBinOp::IntShr
        | SsaBinOp::IntUshr
        | SsaBinOp::FloatAdd
        | SsaBinOp::FloatSub
        | SsaBinOp::FloatMul
        | SsaBinOp::FloatDiv
        | SsaBinOp::FloatMod
        | SsaBinOp::FloatPow
        | SsaBinOp::FloatEq
        | SsaBinOp::FloatNe
        | SsaBinOp::FloatLt
        | SsaBinOp::FloatLe
        | SsaBinOp::FloatGt
        | SsaBinOp::FloatGe
        | SsaBinOp::StrConcat
        | SsaBinOp::StrEq
        | SsaBinOp::StrNe
        | SsaBinOp::Dyn(_) => None,
    }
}

pub(super) fn apply_cap(r: (i128, i128), c: &Check) -> Option<(i128, i128)> {
    let (l, h) = r;
    let (l, h) = if c.below {
        (l, h.min(if c.le { c.bound } else { c.bound - 1 }))
    } else {
        (l.max(if c.le { c.bound } else { c.bound + 1 }), h)
    };
    if l > h {
        None
    } else {
        Some((l, h))
    }
}
