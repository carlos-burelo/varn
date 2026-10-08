use super::super::VerifyError;
use crate::node::{TirExpr, TirExprKind};
use crate::resolution::Resolution;
use crate::ty::{BackendTy, TyId, TyListId};
use crate::{TirFunction, TirModule};
use rustc_hash::FxHashSet as HashSet;

pub(super) fn check_ty(m: &TirModule, ty: BackendTy, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    let mut visited = HashSet::default();
    let mut visited_lists = HashSet::default();
    check_ty_recursive(m, ty, e, errors, &mut visited, &mut visited_lists);
}

fn check_ty_recursive(
    m: &TirModule,
    ty: BackendTy,
    e: &TirExpr,
    errors: &mut Vec<VerifyError>,
    visited: &mut HashSet<TyId>,
    visited_lists: &mut HashSet<TyListId>,
) {
    let bad = |what: &str, errors: &mut Vec<VerifyError>| {
        errors.push(VerifyError::new(
            format!("type names {what}, which has no entry"),
            e.span,
        ));
    };
    match ty {
        BackendTy::Class(c) if m.class(c).is_none() => bad(&format!("ClassId({})", c.0), errors),
        BackendTy::Enum(en) if m.enum_info(en).is_none() => {
            bad(&format!("EnumId({})", en.0), errors)
        }
        BackendTy::Fn(s) if m.signature(s).is_none() => bad(&format!("SigId({})", s.0), errors),
        BackendTy::Array(t) | BackendTy::Set(t) | BackendTy::Nullable(t) => {
            if !m.types.contains(t) {
                bad(&format!("TyId({})", t.0), errors);
            } else if visited.insert(t) {
                let inner_ty = m.types.get(t);
                check_ty_recursive(m, inner_ty, e, errors, visited, visited_lists);
            }
        }
        BackendTy::Map(k, v) => {
            if !m.types.contains(k) {
                bad(&format!("TyId({})", k.0), errors);
            } else if visited.insert(k) {
                let inner_ty = m.types.get(k);
                check_ty_recursive(m, inner_ty, e, errors, visited, visited_lists);
            }
            if !m.types.contains(v) {
                bad(&format!("TyId({})", v.0), errors);
            } else if visited.insert(v) {
                let inner_ty = m.types.get(v);
                check_ty_recursive(m, inner_ty, e, errors, visited, visited_lists);
            }
        }
        BackendTy::Tuple(l) => {
            if !m.types.contains_list(l) {
                bad(&format!("TyListId({})", l.0), errors);
            } else if visited_lists.insert(l) {
                for &elem_ty in m.types.get_list(l) {
                    check_ty_recursive(m, elem_ty, e, errors, visited, visited_lists);
                }
            }
        }
        BackendTy::Int
        | BackendTy::Float
        | BackendTy::Bool
        | BackendTy::Char
        | BackendTy::Str
        | BackendTy::Bytes
        | BackendTy::Decimal
        | BackendTy::BigInt
        | BackendTy::Class(_)
        | BackendTy::Enum(_)
        | BackendTy::Fn(_)
        | BackendTy::Void
        | BackendTy::Never
        | BackendTy::Dynamic(_) => {}
    }
}

pub(super) fn check_res(
    m: &TirModule,
    f: &TirFunction,
    e: &TirExpr,
    errors: &mut Vec<VerifyError>,
) {
    let receiver_class = |recv: &TirExpr| match recv.ty.non_nullable(&m.types) {
        BackendTy::Class(c) => Some(c),
        BackendTy::Int
        | BackendTy::Float
        | BackendTy::Bool
        | BackendTy::Char
        | BackendTy::Str
        | BackendTy::Bytes
        | BackendTy::Decimal
        | BackendTy::BigInt
        | BackendTy::Array(_)
        | BackendTy::Map(..)
        | BackendTy::Set(_)
        | BackendTy::Tuple(_)
        | BackendTy::Enum(_)
        | BackendTy::Fn(_)
        | BackendTy::Nullable(_)
        | BackendTy::Void
        | BackendTy::Never
        | BackendTy::Dynamic(_) => None,
    };

    match &e.res {
        Resolution::FieldSlot(slot) => {
            if let TirExprKind::Field { object, .. } = &e.kind {
                match receiver_class(object) {
                    Some(c) => {
                        let ok = m.class(c).and_then(|ci| ci.field_at(*slot)).is_some();
                        if !ok {
                            errors.push(VerifyError::new(
                                format!(
                                    "field slot {slot} is out of range for class ClassId({})",
                                    c.0
                                ),
                                e.span,
                            ));
                        }
                    }
                    None => errors.push(VerifyError::new(
                        format!("field slot {slot} on a receiver that is not a class"),
                        e.span,
                    )),
                }
            }
        }
        Resolution::VtableSlot(slot) => {
            if let TirExprKind::MethodCall { recv, .. } = &e.kind {
                match receiver_class(recv) {
                    Some(c) => {
                        let ok = m.class(c).and_then(|ci| ci.method_at(*slot)).is_some();
                        if !ok {
                            errors.push(VerifyError::new(
                                format!(
                                    "vtable slot {slot} is out of range for class ClassId({})",
                                    c.0
                                ),
                                e.span,
                            ));
                        }
                    }
                    None => errors.push(VerifyError::new(
                        format!("vtable slot {slot} on a receiver that is not a class"),
                        e.span,
                    )),
                }
            }
        }
        Resolution::GlobalSlot(slot) => {
            if *slot as usize >= m.globals.len() {
                errors.push(VerifyError::new(
                    format!("global slot {slot} is out of range"),
                    e.span,
                ));
            }
        }
        Resolution::DirectFn(f_id) => {
            if m.function(*f_id).is_none() {
                errors.push(VerifyError::new(
                    format!("DirectFn names FnId({}), which has no entry", f_id.0),
                    e.span,
                ));
            }
        }
        Resolution::EnumVariant { enum_id, tag } => {
            let ok = m
                .enum_info(*enum_id)
                .and_then(|ei| ei.variant_at(*tag))
                .is_some();
            if !ok {
                errors.push(VerifyError::new(
                    format!("EnumId({}) has no variant with tag {tag}", enum_id.0),
                    e.span,
                ));
            }
        }
        Resolution::Local(id) => {
            if id.0 as usize >= f.locals.len() {
                errors.push(VerifyError::new(
                    format!("local LocalId({}) is out of range", id.0),
                    e.span,
                ));
            }
        }
        Resolution::Param(id) => {
            if *id as usize >= f.params.len() {
                errors.push(VerifyError::new(
                    format!("parameter at index {id} is out of range"),
                    e.span,
                ));
            }
        }
        Resolution::ByName { name, .. } => {
            if !matches!(
                e.kind,
                TirExprKind::Field { .. } | TirExprKind::MethodCall { .. }
            ) {
                errors.push(VerifyError::new(
                    format!("by-name resolution of `{name}` outside a dynamic member access"),
                    e.span,
                ));
            }
        }
        Resolution::StaticField(_)
        | Resolution::ModuleSlot { .. }
        | Resolution::Intrinsic(_)
        | Resolution::NativeOp(_)
        | Resolution::NativeGlobal(_)
        | Resolution::Upvalue(_)
        | Resolution::None => {}
    }
}
