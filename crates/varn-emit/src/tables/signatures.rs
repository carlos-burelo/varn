use crate::ty::lower_type;
use crate::ty::NameResolver;
use varn_core::{AtomInterner, TypeKind};
use varn_sem::types::{CheckerTyTable, Type};
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
    names: &dyn NameResolver,
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
                        BackendTy::Literal(_)
                        | BackendTy::Int
                        | BackendTy::Float
                        | BackendTy::Bool
                        | BackendTy::Char
                        | BackendTy::Str
                        | BackendTy::Bytes
                        | BackendTy::Decimal
                        | BackendTy::BigInt
                        | BackendTy::Array(_)
                        | BackendTy::Map(..)
                        | BackendTy::Dict(_)
                        | BackendTy::Set(_)
                        | BackendTy::Tuple(_)
                        | BackendTy::Class(_)
                        | BackendTy::Enum(_)
                        | BackendTy::Fn(_)
                        | BackendTy::Void
                        | BackendTy::Never => inner,
                    }
                })
                .collect();
            let return_ty = lower_type(&Type::resolved(ft.return_type), table, interner, tt, names);
            let has_rest = ft.params.last().is_some_and(|p| p.is_rest);
            (p_tys, return_ty, has_rest)
        }
        TypeKind::Primitive(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Named(..)
        | TypeKind::Generic(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Object(_)
        | TypeKind::Record(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => (
            vec![],
            BackendTy::Dynamic(varn_tir::DynReason::Unrecorded),
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

pub(crate) fn accessor_signature(
    kind: varn_core::AccessorKind,
    property: &Type,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
    signatures: &mut Vec<Signature>,
) -> varn_tir::SigId {
    let value = lower_type(property, table, interner, tt, names);
    let (params, return_ty) = match kind {
        varn_core::AccessorKind::Get => (vec![], value),
        varn_core::AccessorKind::Set => (vec![value], BackendTy::Void),
    };
    let id = signatures.len() as u32;
    signatures.push(Signature {
        params,
        return_ty,
        has_rest: false,
    });
    varn_tir::SigId(id)
}
