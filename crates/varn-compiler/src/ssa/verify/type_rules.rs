use super::structural::VerifyResult;
use crate::hir::HirType;
use crate::ssa::ir::{Inst, SsaFunc};

pub(super) fn check_inst_types(func: &SsaFunc, inst: &Inst) -> VerifyResult {
    use crate::ssa::ir::InstKind;
    match &inst.kind {
        InstKind::Binary { op, lhs, rhs, ty } => {
            if *ty != HirType::Dynamic {
                let lty = func.value_ty(*lhs);
                let rty = func.value_ty(*rhs);
                if lty != *ty {
                    return Err(format!(
                        "typed binary {op:?}.{ty:?} expects lhs type {ty:?}, got v{}: {lty:?}",
                        lhs.0
                    ));
                }
                if rty != *ty {
                    return Err(format!(
                        "typed binary {op:?}.{ty:?} expects rhs type {ty:?}, got v{}: {rty:?}",
                        rhs.0
                    ));
                }
                if let Some(dest) = inst.dest {
                    let dty = func.value_ty(dest);
                    let expected_res = crate::hir::binary_result_ty(*op, *ty);
                    let is_str_concat = matches!(op, crate::hir::HirBinOp::Add)
                        && (lty == HirType::Str || rty == HirType::Str)
                        && dty == HirType::Str;
                    if dty != expected_res && !is_str_concat {
                        return Err(format!(
                            "typed binary {op:?}.{ty:?} defines v{}: {dty:?}, expected {expected_res:?}",
                            dest.0
                        ));
                    }
                }
            }
        }
        InstKind::Unary { op, operand, ty } => {
            if matches!(op, crate::hir::HirUnOp::Neg | crate::hir::HirUnOp::BitNot)
                && *ty != HirType::Dynamic
            {
                let oty = func.value_ty(*operand);
                if oty != *ty {
                    return Err(format!(
                        "typed unary {op:?}.{ty:?} expects operand type {ty:?}, got v{}: {oty:?}",
                        operand.0
                    ));
                }
            }
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                let expected = match op {
                    crate::hir::HirUnOp::Not => HirType::Bool,
                    crate::hir::HirUnOp::Typeof => HirType::Str,
                    crate::hir::HirUnOp::Neg | crate::hir::HirUnOp::BitNot => *ty,
                };
                if dty != expected && expected != HirType::Dynamic {
                    return Err(format!(
                        "typed unary {op:?}.{ty:?} defines v{}: {dty:?}, expected {expected:?}",
                        dest.0
                    ));
                }
            }
        }
        InstKind::ConstInt(_) => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != HirType::Int {
                    return Err(format!(
                        "ConstInt defines v{}: {dty:?}, expected Int",
                        dest.0
                    ));
                }
            }
        }
        InstKind::ConstFloat(_) => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != HirType::Float {
                    return Err(format!(
                        "ConstFloat defines v{}: {dty:?}, expected Float",
                        dest.0
                    ));
                }
            }
        }
        InstKind::ConstBool(_) => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != HirType::Bool {
                    return Err(format!(
                        "ConstBool defines v{}: {dty:?}, expected Bool",
                        dest.0
                    ));
                }
            }
        }
        InstKind::ConstStr(_) => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != HirType::Str {
                    return Err(format!(
                        "ConstStr defines v{}: {dty:?}, expected Str",
                        dest.0
                    ));
                }
            }
        }
        InstKind::ConstChar(_) => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != HirType::Ref {
                    return Err(format!(
                        "ConstChar defines v{}: {dty:?}, expected Ref",
                        dest.0
                    ));
                }
            }
        }
        InstKind::IsNull { .. } => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != HirType::Bool {
                    return Err(format!(
                        "IsNull defines v{}: {dty:?}, expected Bool",
                        dest.0
                    ));
                }
            }
        }
        InstKind::Cast { ty, .. } => {
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != *ty {
                    return Err(format!(
                        "Cast defines v{}: {dty:?}, expected {ty:?}",
                        dest.0
                    ));
                }
            }
        }
        InstKind::Convert { operand, conv } => {
            let expected = convert_result_ty(*conv);
            if let Some(dest) = inst.dest {
                let dty = func.value_ty(dest);
                if dty != expected {
                    return Err(format!(
                        "Convert {conv:?} defines v{}: {dty:?}, expected {expected:?}",
                        dest.0
                    ));
                }
            }
            let oty = func.value_ty(*operand);
            if let Some(want) = convert_operand_ty(*conv) {
                if oty != want {
                    return Err(format!(
                        "Convert {conv:?} reads v{}: {oty:?}, expected {want:?}",
                        operand.0
                    ));
                }
            }
        }
        InstKind::ArrayGetIndex { index, .. } | InstKind::ArraySetIndex { index, .. } => {
            let ity = func.value_ty(*index);
            if ity != HirType::Int && ity != HirType::Dynamic {
                return Err(format!(
                    "Array index expects Int or Dynamic, got v{}: {ity:?}",
                    index.0
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn convert_result_ty(conv: varn_core::NumConv) -> HirType {
    use varn_core::NumConv::*;
    match conv {
        IntToFloat | DynToFloat | BigIntToFloat | DecimalToFloat => HirType::Float,
        FloatToInt | BigIntToInt | DecimalToInt | DynToInt => HirType::Int,
        IntToBigInt | IntToDecimal | FloatToBigInt | FloatToDecimal | BigIntToDecimal
        | DecimalToBigInt => HirType::Dynamic,
    }
}

fn convert_operand_ty(conv: varn_core::NumConv) -> Option<HirType> {
    use varn_core::NumConv::*;
    match conv {
        IntToFloat => Some(HirType::Int),
        FloatToInt | FloatToBigInt | FloatToDecimal => Some(HirType::Float),
        IntToBigInt | IntToDecimal | BigIntToInt | DecimalToInt | DynToInt | DynToFloat
        | BigIntToFloat | DecimalToFloat | BigIntToDecimal | DecimalToBigInt => None,
    }
}
