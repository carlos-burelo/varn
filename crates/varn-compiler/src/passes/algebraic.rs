use rustc_hash::FxHashMap;

use crate::hir::{HirBinOp, HirType};
use crate::ssa::ir::{Inst, InstKind, SsaFunc, Value};
use crate::ssa::uses::replace_uses_with_map;

pub fn run(func: &mut SsaFunc) -> bool {
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

    let mut rewrites: FxHashMap<Value, Value> = FxHashMap::default();
    let mut to_const: Vec<(usize, usize, InstKind)> = Vec::new();

    for (b, block) in func.blocks.iter().enumerate() {
        for (i, inst) in block.insts.iter().enumerate() {
            match simplify(inst, &int_const, &float_const) {
                Some(Simplified::Use(v)) => {
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
    Use(Value),

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

                    (_, Some(0)) | (Some(0), _) => Some(Simplified::Const(InstKind::ConstInt(0))),
                    _ => None,
                },

                HirBinOp::Div
                | HirBinOp::Mod
                | HirBinOp::Pow
                | HirBinOp::Eq
                | HirBinOp::Ne
                | HirBinOp::Lt
                | HirBinOp::Le
                | HirBinOp::Gt
                | HirBinOp::Ge
                | HirBinOp::BitAnd
                | HirBinOp::BitOr
                | HirBinOp::BitXor
                | HirBinOp::Shl
                | HirBinOp::Shr
                | HirBinOp::Ushr
                | HirBinOp::Instanceof
                | HirBinOp::In => None,
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
                HirBinOp::Add
                | HirBinOp::Sub
                | HirBinOp::Mod
                | HirBinOp::Pow
                | HirBinOp::Eq
                | HirBinOp::Ne
                | HirBinOp::Lt
                | HirBinOp::Le
                | HirBinOp::Gt
                | HirBinOp::Ge
                | HirBinOp::BitAnd
                | HirBinOp::BitOr
                | HirBinOp::BitXor
                | HirBinOp::Shl
                | HirBinOp::Shr
                | HirBinOp::Ushr
                | HirBinOp::Instanceof
                | HirBinOp::In => None,
            }
        }
        HirType::Bool
        | HirType::Str
        | HirType::Ref
        | HirType::Dynamic
        | HirType::Array(_)
        | HirType::Map(..)
        | HirType::Set(_)
        | HirType::Class(_)
        | HirType::Nullable(_) => None,
    }
}

fn const_inst_ty(kind: &InstKind) -> Option<HirType> {
    match kind {
        InstKind::ConstInt(_) => Some(HirType::Int),
        InstKind::ConstFloat(_) => Some(HirType::Float),
        InstKind::ConstBool(_) => Some(HirType::Bool),
        InstKind::ConstStr(_) => Some(HirType::Str),
        InstKind::ConstChar(_) => Some(HirType::Ref),
        InstKind::ConstDecimal(_) | InstKind::ConstBigInt(_) => Some(HirType::Dynamic),
        InstKind::ConstNull => Some(HirType::Dynamic),
        InstKind::Binary { .. }
        | InstKind::Unary { .. }
        | InstKind::LoadGlobal(_)
        | InstKind::LoadGlobalIdx(_)
        | InstKind::LoadNativeGlobalIdx(_)
        | InstKind::LoadUpvalue(_)
        | InstKind::StoreGlobal { .. }
        | InstKind::StoreGlobalIdx { .. }
        | InstKind::StoreUpvalue { .. }
        | InstKind::Call { .. }
        | InstKind::AllocInstance { .. }
        | InstKind::SelfCall { .. }
        | InstKind::GetProperty { .. }
        | InstKind::GetFixedField { .. }
        | InstKind::GetIndex { .. }
        | InstKind::ArrayGetIndex { .. }
        | InstKind::MapGetIndex { .. }
        | InstKind::SetProperty { .. }
        | InstKind::SetFixedField { .. }
        | InstKind::SetIndex { .. }
        | InstKind::ArraySetIndex { .. }
        | InstKind::MapSetIndex { .. }
        | InstKind::ArrayPush { .. }
        | InstKind::ObjectMerge { .. }
        | InstKind::MethodCall { .. }
        | InstKind::IsNull { .. }
        | InstKind::Cast { .. }
        | InstKind::Convert { .. }
        | InstKind::BuildArray { .. }
        | InstKind::BuildTuple { .. }
        | InstKind::BuildObject { .. }
        | InstKind::BuildRecord { .. }
        | InstKind::BuildMap { .. }
        | InstKind::ObjectRest { .. }
        | InstKind::ToString { .. }
        | InstKind::BuildStr { .. }
        | InstKind::MakeClosure { .. }
        | InstKind::LoadCaptured { .. }
        | InstKind::StoreCaptured { .. }
        | InstKind::MakeClass { .. }
        | InstKind::DeclareLayout { .. }
        | InstKind::DefineStatic { .. }
        | InstKind::DefineMethod { .. }
        | InstKind::DefineAccessor { .. }
        | InstKind::MakeEnumVariant { .. }
        | InstKind::Try { .. }
        | InstKind::PopTry
        | InstKind::CatchParam { .. }
        | InstKind::CloseUpvalues { .. }
        | InstKind::Dispose { .. }
        | InstKind::LoadModule { .. }
        | InstKind::StoreModuleSlot { .. }
        | InstKind::Await { .. }
        | InstKind::Spawn { .. }
        | InstKind::Yield { .. }
        | InstKind::IntrinsicCall { .. }
        | InstKind::CallNativeOp { .. }
        | InstKind::AssertNotNull { .. }
        | InstKind::GetPropertyMaybe { .. }
        | InstKind::ModuleSlot { .. }
        | InstKind::GetEnumTag { .. }
        | InstKind::IsArray { .. }
        | InstKind::StrLength { .. }
        | InstKind::ArrayLength { .. }
        | InstKind::BytesLength { .. }
        | InstKind::This
        | InstKind::Range { .. }
        | InstKind::ObjectKeys { .. }
        | InstKind::GetSymbol { .. }
        | InstKind::IterCall { .. }
        | InstKind::GetSuper { .. }
        | InstKind::SuperCall { .. }
        | InstKind::SuperMethodCall { .. }
        | InstKind::ExtensionCall { .. }
        | InstKind::CallSpread { .. }
        | InstKind::BuildArraySpread { .. }
        | InstKind::BuildObjectSpread { .. } => None,
    }
}
