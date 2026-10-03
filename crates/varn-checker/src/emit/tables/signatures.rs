use super::NameIndex;
use crate::emit::ty::lower_type;
use crate::types::{CheckerTyTable, Type};
use varn_core::{AtomInterner, TypeKind};
use varn_tir::{BackendTy, Signature, TyTable};

pub(super) fn seed_signatures() -> Vec<Signature> {
    vec![Signature {
        params: vec![],
        return_ty: BackendTy::Void,
    }]
}

pub(crate) fn intern_signature(
    ty: &Type,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    signatures: &mut Vec<Signature>,
) -> varn_tir::SigId {
    let (params, return_ty) = match table.get(ty.0) {
        TypeKind::Fn(fid) => {
            let ft = table.get_function(fid);
            let p_tys: Vec<BackendTy> = ft
                .params
                .iter()
                .map(|p| {
                    let inner = lower_type(&Type(p.ty, false), table, interner, tt, names);
                    match inner {
                        BackendTy::Dynamic(_) | BackendTy::Nullable(_) => inner,
                        _ if p.optional => BackendTy::Nullable(tt.intern(inner)),
                        _ => inner,
                    }
                })
                .collect();
            let return_ty = lower_type(&Type(ft.return_type, false), table, interner, tt, names);
            (p_tys, return_ty)
        }
        _ => (vec![], BackendTy::Dynamic(varn_tir::DynReason::Unannotated)),
    };
    let id = signatures.len() as u32;
    signatures.push(Signature { params, return_ty });
    varn_tir::SigId(id)
}
