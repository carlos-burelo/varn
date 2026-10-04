use super::NameIndex;
use crate::emit::ty::lower_type;
use crate::types::{CheckerTyTable, Type};
use varn_core::{AtomInterner, TypeKind};
use varn_tir::{BackendTy, Signature, TyTable};

pub(super) fn seed_signatures() -> Vec<Signature> {
    vec![Signature {
        params: vec![],
        return_ty: BackendTy::Void,
        has_rest: false,
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
    let (params, return_ty, has_rest) = match table.get(ty.0) {
        TypeKind::Fn(fid) => {
            let ft = table.get_function(fid);
            let p_tys: Vec<BackendTy> = ft
                .params
                .iter()
                .map(|p| {
                    let inner = lower_type(&Type::resolved(p.ty), table, interner, tt, names);
                    match inner {
                        BackendTy::Dynamic(_) | BackendTy::Nullable(_) => inner,
                        _ if p.optional => BackendTy::Nullable(tt.intern(inner)),
                        _ => inner,
                    }
                })
                .collect();
            let return_ty = lower_type(&Type::resolved(ft.return_type), table, interner, tt, names);
            let has_rest = ft.params.last().is_some_and(|p| p.is_rest);
            (p_tys, return_ty, has_rest)
        }
        _ => (
            vec![],
            BackendTy::Dynamic(varn_tir::DynReason::NotYetSupported),
            false,
        ),
    };
    let id = signatures.len() as u32;
    signatures.push(Signature {
        params,
        return_ty,
        has_rest,
    });
    varn_tir::SigId(id)
}
