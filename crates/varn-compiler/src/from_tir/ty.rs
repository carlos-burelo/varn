use crate::hir::{HirType, TyTable as SsaTyTable};
use varn_tir::{BackendTy, TirModule};

pub fn lower(bt: BackendTy, tir: &TirModule, out: &mut SsaTyTable) -> HirType {
    match bt {
        BackendTy::Int => HirType::Int,
        BackendTy::Float => HirType::Float,
        BackendTy::Bool => HirType::Bool,
        BackendTy::Str => HirType::Str,

        BackendTy::Char => HirType::Ref,

        BackendTy::Decimal | BackendTy::BigInt => HirType::Dynamic,

        BackendTy::Tuple(_) => HirType::Tuple,

        BackendTy::Bytes | BackendTy::Enum(_) | BackendTy::Fn(_) => HirType::Ref,

        BackendTy::Dict(v) => {
            let v_inner = resolve(v, tir, out);
            HirType::Map(out.intern(HirType::Dynamic), out.intern(v_inner))
        }
        BackendTy::Map(k, v) => {
            let k_inner = resolve(k, tir, out);
            let v_inner = resolve(v, tir, out);
            HirType::Map(out.intern(k_inner), out.intern(v_inner))
        }
        BackendTy::Set(el) => {
            let inner = resolve(el, tir, out);
            HirType::Set(out.intern(inner))
        }

        BackendTy::Array(el) => {
            let inner = resolve(el, tir, out);
            HirType::Array(out.intern(inner))
        }
        BackendTy::Class(cid) => match tir.class(cid) {
            Some(ci) => HirType::Class(out.class_id(&ci.name)),
            None => {
                debug_assert!(false, "from_tir: ClassId({}) missing after verify", cid.0);
                HirType::Ref
            }
        },
        BackendTy::Nullable(inner) => {
            let inner = resolve(inner, tir, out);
            HirType::Nullable(out.intern(inner))
        }

        BackendTy::Void | BackendTy::Never | BackendTy::Dynamic(_) => HirType::Dynamic,
    }
}

fn resolve(id: varn_tir::TyId, tir: &TirModule, out: &mut SsaTyTable) -> HirType {
    if !tir.types.contains(id) {
        debug_assert!(false, "from_tir: TyId({}) missing after verify", id.0);
        return HirType::Dynamic;
    }
    lower(tir.types.get(id), tir, out)
}
