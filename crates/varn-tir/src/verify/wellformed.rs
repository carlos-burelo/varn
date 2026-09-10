//! Every handle points at something that exists; every slot is in range.

use super::VerifyError;
use crate::node::{TirExpr, TirExprKind, TirFunction, TirModule, TirStmt};
use crate::resolution::Resolution;
use crate::ty::{BackendTy, TyId, TyListId};
use std::collections::HashSet;

pub(super) fn check(m: &TirModule, errors: &mut Vec<VerifyError>) {
    check_declarations(m, errors);
    check_function(m, &m.top_level, errors);
    for f in &m.functions {
        check_function(m, f, errors);
    }
}

fn check_declarations(m: &TirModule, errors: &mut Vec<VerifyError>) {
    // Check globals
    let dummy_expr = TirExpr {
        kind: TirExprKind::NullLit,
        ty: BackendTy::Void,
        res: Resolution::None,
        span: crate::node::Span::EMPTY,
    };
    for ty in &m.globals {
        check_ty(m, *ty, &dummy_expr, errors);
    }

    // Check class fields
    for class in &m.classes {
        for field in &class.fields {
            check_ty(m, field.ty, &dummy_expr, errors);
        }
        // Every vtable entry must name a real signature — a dangling SigId
        // here silently disables check_method_call's arity/type coherence
        // rule (it looks up the signature and just returns if absent).
        for entry in &class.vtable {
            if m.signature(entry.sig).is_none() {
                errors.push(VerifyError::new(
                    format!(
                        "vtable entry `{}` names SigId({}), which has no entry",
                        entry.name, entry.sig.0
                    ),
                    dummy_expr.span,
                ));
            }
        }
    }

    // Check enum variants
    for enum_info in &m.enums {
        for variant in &enum_info.variants {
            for ty in &variant.payload {
                check_ty(m, *ty, &dummy_expr, errors);
            }
        }
    }

    // Check signatures
    for sig in &m.signatures {
        for param_ty in &sig.params {
            check_ty(m, *param_ty, &dummy_expr, errors);
        }
        check_ty(m, sig.return_ty, &dummy_expr, errors);
    }
}

fn check_function(m: &TirModule, f: &TirFunction, errors: &mut Vec<VerifyError>) {
    if m.signature(f.sig).is_none() {
        errors.push(VerifyError::new(
            format!(
                "function `{}` names SigId({}), which has no entry",
                f.name, f.sig.0
            ),
            crate::node::Span::EMPTY,
        ));
    }

    // Check function's own type declarations
    let dummy_expr = TirExpr {
        kind: TirExprKind::NullLit,
        ty: BackendTy::Void,
        res: Resolution::None,
        span: crate::node::Span::EMPTY,
    };
    for param_ty in &f.params {
        check_ty(m, *param_ty, &dummy_expr, errors);
    }
    check_ty(m, f.return_ty, &dummy_expr, errors);
    for local_ty in &f.locals {
        check_ty(m, *local_ty, &dummy_expr, errors);
    }

    for s in &f.body {
        check_stmt(m, f, s, errors);
    }
}

fn check_stmt(m: &TirModule, f: &TirFunction, s: &TirStmt, errors: &mut Vec<VerifyError>) {
    match s {
        TirStmt::Expr(e) | TirStmt::Throw(e) => check_expr(m, f, e, errors),
        TirStmt::Let {
            ty, init, local, ..
        } => {
            let dummy_expr = TirExpr {
                kind: TirExprKind::NullLit,
                ty: BackendTy::Void,
                res: Resolution::None,
                span: crate::node::Span::EMPTY,
            };
            check_ty(m, *ty, &dummy_expr, errors);
            if local.0 as usize >= f.locals.len() {
                errors.push(VerifyError::new(
                    format!("Let binds LocalId({}), which is out of range", local.0),
                    crate::node::Span::EMPTY,
                ));
            }
            if let Some(e) = init {
                check_expr(m, f, e, errors);
            }
        }
        TirStmt::Return(v) => {
            if let Some(e) = v {
                check_expr(m, f, e, errors);
            }
        }
        TirStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            check_expr(m, f, cond, errors);
            for s in then_body.iter().chain(else_body) {
                check_stmt(m, f, s, errors);
            }
        }
        TirStmt::Loop { cond, body } => {
            check_expr(m, f, cond, errors);
            for s in body {
                check_stmt(m, f, s, errors);
            }
        }
        TirStmt::Try {
            body,
            catch_local,
            catch_body,
        } => {
            if catch_local.0 as usize >= f.locals.len() {
                errors.push(VerifyError::new(
                    format!(
                        "Try binds catch LocalId({}), which is out of range",
                        catch_local.0
                    ),
                    crate::node::Span::EMPTY,
                ));
            }
            for s in body.iter().chain(catch_body) {
                check_stmt(m, f, s, errors);
            }
        }
        TirStmt::Break | TirStmt::Continue | TirStmt::BuildClass(_) => {}
    }
}

fn check_expr(m: &TirModule, f: &TirFunction, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    check_ty(m, e.ty, e, errors);
    check_res(m, f, e, errors);

    match &e.kind {
        TirExprKind::Binary { lhs, rhs, .. } => {
            check_expr(m, f, lhs, errors);
            check_expr(m, f, rhs, errors);
        }
        TirExprKind::Unary { operand, .. } | TirExprKind::Cast { operand } => {
            check_expr(m, f, operand, errors)
        }
        TirExprKind::Field { object, .. } => check_expr(m, f, object, errors),
        TirExprKind::Index { object, index } => {
            check_expr(m, f, object, errors);
            check_expr(m, f, index, errors);
        }
        TirExprKind::Call { callee, args } => {
            check_expr(m, f, callee, errors);
            for a in args {
                check_expr(m, f, a.value(), errors);
            }
        }
        TirExprKind::MethodCall { recv, args, .. } => {
            check_expr(m, f, recv, errors);
            for a in args {
                check_expr(m, f, a.value(), errors);
            }
        }
        TirExprKind::Assign { target, value } => {
            check_expr(m, f, target, errors);
            check_expr(m, f, value, errors);
        }
        TirExprKind::TupleLit(xs) => {
            for x in xs {
                check_expr(m, f, x, errors);
            }
        }
        TirExprKind::RecordLit { fields } => {
            for (_, v) in fields {
                check_expr(m, f, v, errors);
            }
        }
        TirExprKind::ArrayLit(els) => {
            for el in els {
                match el {
                    crate::node::TirArrayEl::Expr(x) | crate::node::TirArrayEl::Spread(x) => {
                        check_expr(m, f, x, errors)
                    }
                    crate::node::TirArrayEl::Hole => {}
                }
            }
        }
        TirExprKind::ObjectLit { entries } => {
            for entry in entries {
                match entry {
                    crate::node::TirObjectEntry::Field { value, .. }
                    | crate::node::TirObjectEntry::Spread(value) => check_expr(m, f, value, errors),
                }
            }
        }
        TirExprKind::Closure { func, .. } => {
            if m.function(*func).is_none() {
                errors.push(VerifyError::new(
                    format!("Closure names FnId({}), which has no entry", func.0),
                    e.span,
                ));
            }
        }
        TirExprKind::New { class, args } => {
            if m.class(*class).is_none() {
                errors.push(VerifyError::new(
                    format!("New names ClassId({}), which has no entry", class.0),
                    e.span,
                ));
            }
            for a in args {
                check_expr(m, f, a.value(), errors);
            }
        }
        TirExprKind::MakeVariant { args } => {
            for a in args {
                check_expr(m, f, a.value(), errors);
            }
        }
        TirExprKind::Await { future } => check_expr(m, f, future, errors),
        TirExprKind::Yield { value, .. } => {
            if let Some(v) = value {
                check_expr(m, f, v, errors);
            }
        }
        TirExprKind::Discriminant { value } => check_expr(m, f, value, errors),
        TirExprKind::VariantPayload { value, .. } => check_expr(m, f, value, errors),
        TirExprKind::TypeTest { value, class } => {
            if m.class(*class).is_none() {
                errors.push(VerifyError::new(
                    format!("TypeTest names ClassId({}), which has no entry", class.0),
                    e.span,
                ));
            }
            check_expr(m, f, value, errors);
        }
        TirExprKind::Select {
            cond,
            then_val,
            else_val,
        } => {
            check_expr(m, f, cond, errors);
            check_expr(m, f, then_val, errors);
            check_expr(m, f, else_val, errors);
        }
        TirExprKind::ObjectKeys { operand } => check_expr(m, f, operand, errors),
        TirExprKind::IterInit { source, .. } => check_expr(m, f, source, errors),
        TirExprKind::SuperCall { args } | TirExprKind::SuperMethodCall { args, .. } => {
            for a in args {
                check_expr(m, f, a.value(), errors);
            }
        }
        TirExprKind::RangeLit { start, end, .. } => {
            check_expr(m, f, start, errors);
            check_expr(m, f, end, errors);
        }
        TirExprKind::DecimalLit(_) | TirExprKind::BigIntLit(_) => {}
        TirExprKind::ObjectRest { object, .. } => check_expr(m, f, object, errors),
        TirExprKind::ExtensionCall { recv, args, .. } => {
            check_expr(m, f, recv, errors);
            for a in args {
                check_expr(m, f, a.value(), errors);
            }
        }
        TirExprKind::IntLit(_)
        | TirExprKind::FloatLit(_)
        | TirExprKind::BoolLit(_)
        | TirExprKind::StrLit(_)
        | TirExprKind::CharLit(_)
        | TirExprKind::NullLit
        | TirExprKind::Var => {}
    }
}

/// Every handle inside a type points at an entry that exists, and types do not form cycles.
fn check_ty(m: &TirModule, ty: BackendTy, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    let mut visited = HashSet::new();
    let mut visited_lists = HashSet::new();
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
                // First time seeing this TyId, recurse into it
                let inner_ty = m.types.get(t);
                check_ty_recursive(m, inner_ty, e, errors, visited, visited_lists);
            }
            // If already visited, stop to break cycles
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
                // First time seeing this TyListId — recurse into its
                // elements. A Tuple can point back at its own TyListId
                // (e.g. `intern_list(&[Tuple(TyListId(0))])` on an empty
                // table), which is a genuine self-reference distinct from
                // any TyId cycle, so it needs its own visited set.
                for &elem_ty in m.types.get_list(l) {
                    check_ty_recursive(m, elem_ty, e, errors, visited, visited_lists);
                }
            }
            // If already visited, stop to break cycles
        }
        _ => {}
    }
}

/// Every slot is in range of the table it claims to index.
fn check_res(m: &TirModule, f: &TirFunction, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    let receiver_class = |recv: &TirExpr| match recv.ty.non_nullable(&m.types) {
        BackendTy::Class(c) => Some(c),
        _ => None,
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
        // StaticField, ModuleSlot, Intrinsic, NativeOp, Upvalue, ByName, and None
        // have no backing tables in this crate and cannot be validated here
        Resolution::StaticField(_)
        | Resolution::ModuleSlot { .. }
        | Resolution::Intrinsic(_)
        | Resolution::NativeOp(_)
        | Resolution::Upvalue(_)
        | Resolution::ByName { .. }
        | Resolution::None => {}
    }
}
