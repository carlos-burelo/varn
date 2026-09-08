//! Type ↔ operation coherence.
//!
//! A missing type costs performance; a wrong one is a miscompile. Nothing in
//! the pipeline looks for the second today.

use super::VerifyError;
use crate::node::{
    TirArg, TirArrayEl, TirBinOp, TirExpr, TirExprKind, TirFunction, TirModule, TirObjectEntry,
    TirStmt, TirUnOp,
};
use crate::resolution::Resolution;
use crate::ty::BackendTy;

pub(super) fn check(m: &TirModule, errors: &mut Vec<VerifyError>) {
    check_function(m, &m.top_level, errors);
    for f in &m.functions {
        check_function(m, f, errors);
    }
}

fn check_function(m: &TirModule, f: &TirFunction, errors: &mut Vec<VerifyError>) {
    for s in &f.body {
        walk_stmt(m, f, s, errors);
    }
}

fn walk_stmt(m: &TirModule, f: &TirFunction, s: &TirStmt, errors: &mut Vec<VerifyError>) {
    match s {
        TirStmt::Expr(e) | TirStmt::Throw(e) => walk_expr(m, f, e, errors),
        TirStmt::Let { ty, init, .. } => {
            if let Some(e) = init {
                walk_expr(m, f, e, errors);
                check_let(m, *ty, e, errors);
            }
        }
        TirStmt::Return(v) => {
            if let Some(e) = v {
                walk_expr(m, f, e, errors);
                check_return(m, f, e, errors);
            } else {
                // Bare Return(None) requires function to return Void
                check_return_none(m, f, errors);
            }
        }
        TirStmt::If { cond, then_body, else_body } => {
            walk_expr(m, f, cond, errors);
            check_condition(m, "if", cond, errors);
            for s in then_body.iter().chain(else_body) {
                walk_stmt(m, f, s, errors);
            }
        }
        TirStmt::Loop { cond, body } => {
            walk_expr(m, f, cond, errors);
            check_condition(m, "loop", cond, errors);
            for s in body {
                walk_stmt(m, f, s, errors);
            }
        }
        TirStmt::Try { body, catch_body, .. } => {
            for s in body.iter().chain(catch_body) {
                walk_stmt(m, f, s, errors);
            }
        }
        TirStmt::Break | TirStmt::Continue | TirStmt::BuildClass(_) => {}
    }
}

fn walk_arg(m: &TirModule, f: &TirFunction, a: &TirArg, errors: &mut Vec<VerifyError>) {
    walk_expr(m, f, a.value(), errors);
}

fn walk_expr(m: &TirModule, f: &TirFunction, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    match &e.kind {
        TirExprKind::Binary { op, lhs, rhs } => {
            walk_expr(m, f, lhs, errors);
            walk_expr(m, f, rhs, errors);
            check_binary(m, e, *op, lhs, rhs, errors);
        }
        TirExprKind::Field { object, .. } => {
            walk_expr(m, f, object, errors);
            check_field(m, e, object, errors);
        }
        TirExprKind::Index { object, index } => {
            walk_expr(m, f, object, errors);
            walk_expr(m, f, index, errors);
            check_index(m, e, object, errors);
        }
        TirExprKind::Unary { op, operand } => {
            walk_expr(m, f, operand, errors);
            if *op == TirUnOp::IsNull && e.ty != BackendTy::Bool {
                errors.push(VerifyError::new(
                    format!("IsNull must produce Bool, node says {:?}", e.ty),
                    e.span,
                ));
            }
        }
        TirExprKind::Cast { operand } => walk_expr(m, f, operand, errors),
        TirExprKind::Call { callee, args } => {
            walk_expr(m, f, callee, errors);
            for a in args {
                walk_arg(m, f, a, errors);
            }
            check_direct_call(m, e, args, errors);
        }
        TirExprKind::MethodCall { recv, args, .. } => {
            walk_expr(m, f, recv, errors);
            for a in args {
                walk_arg(m, f, a, errors);
            }
            check_method_call(m, e, recv, args, errors);
        }
        TirExprKind::Assign { target, value } => {
            walk_expr(m, f, target, errors);
            walk_expr(m, f, value, errors);
        }
        TirExprKind::TupleLit(xs) => {
            for x in xs {
                walk_expr(m, f, x, errors);
            }
        }
        TirExprKind::ArrayLit(els) => {
            for el in els {
                match el {
                    TirArrayEl::Expr(x) | TirArrayEl::Spread(x) => walk_expr(m, f, x, errors),
                    TirArrayEl::Hole => {}
                }
            }
        }
        TirExprKind::ObjectLit { entries } => {
            for entry in entries {
                match entry {
                    TirObjectEntry::Field { value, .. } | TirObjectEntry::Spread(value) => {
                        walk_expr(m, f, value, errors)
                    }
                }
            }
        }
        TirExprKind::New { args, .. } | TirExprKind::MakeVariant { args } => {
            for a in args {
                walk_arg(m, f, a, errors);
            }
        }
        TirExprKind::Await { future } => {
            walk_expr(m, f, future, errors);
            if !f.is_async {
                errors.push(VerifyError::new(
                    format!("`await` in `{}`, which is not async", f.name),
                    e.span,
                ));
            }
        }
        TirExprKind::Yield { value, .. } => {
            if let Some(v) = value {
                walk_expr(m, f, v, errors);
            }
            if !f.is_generator {
                errors.push(VerifyError::new(
                    format!("`yield` in `{}`, which is not a generator", f.name),
                    e.span,
                ));
            }
        }
        TirExprKind::Discriminant { value } => {
            walk_expr(m, f, value, errors);
            if e.ty != BackendTy::Int {
                errors.push(VerifyError::new(
                    format!("Discriminant must produce Int, node says {:?}", e.ty),
                    e.span,
                ));
            }
            if !matches!(
                value.ty.non_nullable(&m.types),
                BackendTy::Enum(_) | BackendTy::Dynamic(_)
            ) {
                errors.push(VerifyError::new(
                    format!("Discriminant of {:?}, which is not an enum", value.ty),
                    e.span,
                ));
            }
        }
        TirExprKind::VariantPayload { value, tag, field } => {
            walk_expr(m, f, value, errors);
            check_variant_payload(m, e, value, *tag, *field, errors);
        }
        TirExprKind::TypeTest { value, .. } => {
            walk_expr(m, f, value, errors);
            if e.ty != BackendTy::Bool {
                errors.push(VerifyError::new(
                    format!("TypeTest must produce Bool, node says {:?}", e.ty),
                    e.span,
                ));
            }
        }
        TirExprKind::ObjectKeys { operand } => walk_expr(m, f, operand, errors),
        TirExprKind::SuperCall { args } | TirExprKind::SuperMethodCall { args, .. } => {
            for a in args { walk_arg(m, f, a, errors); }
        }
        TirExprKind::RangeLit { start, end, .. } => {
            walk_expr(m, f, start, errors);
            walk_expr(m, f, end, errors);
        }
        TirExprKind::DecimalLit(_) | TirExprKind::BigIntLit(_) => {}
        TirExprKind::ObjectRest { object, .. } => walk_expr(m, f, object, errors),
        TirExprKind::ExtensionCall { recv, args, .. } => {
            walk_expr(m, f, recv, errors);
            for a in args { walk_arg(m, f, a, errors); }
        }
        TirExprKind::Select { cond, then_val, else_val } => {
            walk_expr(m, f, cond, errors);
            walk_expr(m, f, then_val, errors);
            walk_expr(m, f, else_val, errors);
            if then_val.ty != else_val.ty && e.ty != BackendTy::Dynamic(crate::ty::DynReason::Union)
            {
                errors.push(VerifyError::new(
                    "Select arms have different types and the result is not a union",
                    e.span,
                ));
            }
        }
        TirExprKind::IntLit(_)
        | TirExprKind::FloatLit(_)
        | TirExprKind::BoolLit(_)
        | TirExprKind::StrLit(_)
        | TirExprKind::CharLit(_)
        | TirExprKind::NullLit
        | TirExprKind::Closure { .. }
        | TirExprKind::Var => {}
    }
}

/// A payload read produces exactly the variant field's declared type.
fn check_variant_payload(
    m: &TirModule,
    e: &TirExpr,
    value: &TirExpr,
    tag: u16,
    field: u16,
    errors: &mut Vec<VerifyError>,
) {
    let BackendTy::Enum(id) = value.ty.non_nullable(&m.types) else {
        return; // wellformed already reported this
    };
    let Some(variant) = m.enum_info(id).and_then(|ei| ei.variant_at(tag)) else {
        errors.push(VerifyError::new(
            format!("VariantPayload names tag {tag}, which EnumId({}) has no variant for", id.0),
            e.span,
        ));
        return;
    };
    let Some(&field_ty) = variant.payload.get(field as usize) else {
        errors.push(VerifyError::new(
            format!(
                "VariantPayload field {field} is out of range for variant `{}`",
                variant.name
            ),
            e.span,
        ));
        return;
    };
    if e.ty != field_ty {
        errors.push(VerifyError::new(
            format!(
                "variant `{}` field {field} is {:?}, node says {:?}",
                variant.name, field_ty, e.ty
            ),
            e.span,
        ));
    }
}

/// Whether a value of type `from` may be used where `to` is expected.
///
/// Not equality: `T` is assignable to `T?` — a non-null value is a valid
/// nullable — while `T?` to `T` is not, because that needs narrowing. Both
/// sides being Dynamic-tolerant keeps an honestly dynamic value from being
/// reported anywhere.
///
/// Uses a depth bound because coherence runs unconditionally even after
/// wellformed finds errors, so a cyclic type can reach here. Unlike check_ty,
/// this helper cannot rely on wellformed having run first.
fn assignable(m: &TirModule, from: BackendTy, to: BackendTy) -> bool {
    assignable_with_depth(m, from, to, 0)
}

const ASSIGNABLE_DEPTH_LIMIT: usize = 32;

fn assignable_with_depth(
    m: &TirModule,
    from: BackendTy,
    to: BackendTy,
    depth: usize,
) -> bool {
    if depth > ASSIGNABLE_DEPTH_LIMIT {
        // Cycle detected or pathologically deep nesting. Return true so a
        // cyclic type doesn't become a false positive; the real error is in
        // wellformed if the cycle is wrong, not here.
        return true;
    }

    if matches!(from, BackendTy::Dynamic(_)) || matches!(to, BackendTy::Dynamic(_)) {
        return true;
    }
    if from == to {
        return true;
    }
    // Never inhabits every type: a call that always throws can stand in
    // anywhere.
    if from == BackendTy::Never {
        return true;
    }
    // `int` widens implicitly to the other numeric types — this is how a call
    // like `takesFloat(1)` or `takesDecimal(1)` type-checks in the language,
    // so a call argument is assignable across it. Arithmetic stays strict:
    // `check_binary` compares by equality, not through here.
    if from == BackendTy::Int
        && matches!(to, BackendTy::Float | BackendTy::Decimal | BackendTy::BigInt)
    {
        return true;
    }
    // Arrays and sets are covariant in their element for assignability — the
    // checker treats them so, and the backend representation is a pointer
    // either way.
    match (from, to) {
        (BackendTy::Array(a), BackendTy::Array(b)) | (BackendTy::Set(a), BackendTy::Set(b))
            if m.types.contains(a) && m.types.contains(b) =>
        {
            return assignable_with_depth(m, m.types.get(a), m.types.get(b), depth + 1);
        }
        _ => {}
    }
    // A subclass is assignable to any of its ancestors.
    if let (BackendTy::Class(sub), BackendTy::Class(sup)) = (from, to) {
        let mut cur = Some(sub);
        let mut hops = 0;
        while let Some(c) = cur {
            if c == sup {
                return true;
            }
            if hops > ASSIGNABLE_DEPTH_LIMIT {
                break;
            }
            hops += 1;
            cur = m.class(c).and_then(|ci| ci.parent);
        }
    }
    // The bare null value — `Nullable` over a `Never` payload — is assignable
    // to every nullable type. It is what `return null` in a `T?` function
    // produces.
    if let (BackendTy::Nullable(fi), BackendTy::Nullable(_)) = (from, to) {
        if m.types.contains(fi) && m.types.get(fi) == BackendTy::Never {
            return true;
        }
    }
    // T is assignable to T?; the reverse is not.
    if let BackendTy::Nullable(inner) = to {
        // A dangling handle means wellformed already reported the real
        // problem elsewhere; return true (the same safe direction as the
        // depth bound above) rather than indexing blindly.
        if !m.types.contains(inner) {
            return true;
        }
        return assignable_with_depth(m, from, m.types.get(inner), depth + 1);
    }
    false
}

fn is_comparison(op: TirBinOp) -> bool {
    matches!(
        op,
        TirBinOp::Eq | TirBinOp::Ne | TirBinOp::Lt | TirBinOp::Le | TirBinOp::Gt | TirBinOp::Ge
    )
}

fn check_binary(
    m: &TirModule,
    e: &TirExpr,
    op: TirBinOp,
    lhs: &TirExpr,
    rhs: &TirExpr,
    errors: &mut Vec<VerifyError>,
) {
    if is_comparison(op) {
        if e.ty != BackendTy::Bool {
            errors.push(VerifyError::new(
                format!("comparison {op:?} must produce Bool, node says {:?}", e.ty),
                e.span,
            ));
        }
        return;
    }

    let l = lhs.ty.non_nullable(&m.types);
    let r = rhs.ty.non_nullable(&m.types);

    // Dynamic operands make the operation generic; nothing to prove.
    if matches!(l, BackendTy::Dynamic(_)) || matches!(r, BackendTy::Dynamic(_)) {
        return;
    }

    if l != r {
        errors.push(VerifyError::new(
            format!(
                "{op:?} mixes {:?} and {:?}; an explicit Cast is required",
                l, r
            ),
            e.span,
        ));
        return;
    }

    // int / int is the one arithmetic case whose result leaves the operand
    // class, and `varn_core::numeric` is where that rule lives.
    let expected = if op == TirBinOp::Div && l == BackendTy::Int {
        BackendTy::Float
    } else {
        l
    };

    if e.ty != expected {
        errors.push(VerifyError::new(
            format!(
                "{op:?} on {:?} produces {:?}, node says {:?}",
                l, expected, e.ty
            ),
            e.span,
        ));
    }
}

fn check_field(m: &TirModule, e: &TirExpr, object: &TirExpr, errors: &mut Vec<VerifyError>) {
    let Resolution::FieldSlot(slot) = &e.res else {
        return;
    };
    let BackendTy::Class(c) = object.ty.non_nullable(&m.types) else {
        return; // well-formedness already reported this
    };
    let Some(field) = m.class(c).and_then(|ci| ci.field_at(*slot)) else {
        return; // ditto
    };
    // Equality is deliberate here: a field read produces exactly the field's
    // declared type. If the node claims a different type, the nullability or
    // type itself was lost, and that is a real error to report.
    if e.ty != field.ty {
        errors.push(VerifyError::new(
            format!(
                "field `{}` is declared {:?}, node says {:?}",
                field.name, field.ty, e.ty
            ),
            e.span,
        ));
    }
}

fn check_index(m: &TirModule, e: &TirExpr, object: &TirExpr, errors: &mut Vec<VerifyError>) {
    if let BackendTy::Array(el) = object.ty.non_nullable(&m.types) {
        let elem = m.types.get(el);
        if e.ty != elem {
            errors.push(VerifyError::new(
                format!("indexing an array of {:?} must produce {:?}, node says {:?}", elem, elem, e.ty),
                e.span,
            ));
        }
    }
}

/// Arity and per-argument types can only be checked against a positional list
/// with no spread. A spread contributes an unknown count; a named argument is
/// matched by label, not position — both are left to a later rule.
fn is_positional(args: &[TirArg]) -> bool {
    args.iter().all(|a| matches!(a, TirArg::Expr(_)))
}

fn check_direct_call(
    m: &TirModule,
    e: &TirExpr,
    args: &[TirArg],
    errors: &mut Vec<VerifyError>,
) {
    let Resolution::DirectFn(f) = &e.res else {
        return;
    };
    let Some(func) = m.function(*f) else {
        return;
    };
    let Some(sig) = m.signature(func.sig) else {
        return;
    };
    if !is_positional(args) {
        return;
    }
    if args.len() != sig.arity() {
        errors.push(VerifyError::new(
            format!(
                "call to `{}` passes {} arguments, signature takes {}",
                func.name,
                args.len(),
                sig.arity()
            ),
            e.span,
        ));
        return;
    }
    // Each argument must be assignable TO its parameter
    for (i, (a, p)) in args.iter().zip(&sig.params).enumerate() {
        if !assignable(m, a.value().ty, *p) {
            errors.push(VerifyError::new(
                format!(
                    "call to `{}`: argument {i} is {:?}, parameter is {:?}",
                    func.name,
                    a.value().ty,
                    p
                ),
                e.span,
            ));
        }
    }
    // The signature's return type must be assignable TO what the node claims
    if !assignable(m, sig.return_ty, e.ty) {
        errors.push(VerifyError::new(
            format!(
                "call to `{}` returns {:?}, node says {:?}",
                func.name, sig.return_ty, e.ty
            ),
            e.span,
        ));
    }
}

fn check_method_call(
    m: &TirModule,
    e: &TirExpr,
    recv: &TirExpr,
    args: &[TirArg],
    errors: &mut Vec<VerifyError>,
) {
    let Resolution::VtableSlot(slot) = &e.res else {
        return;
    };
    let BackendTy::Class(c) = recv.ty.non_nullable(&m.types) else {
        return; // well-formedness already reported this
    };
    let Some(class_info) = m.class(c) else {
        return; // ditto
    };
    let Some(vtable_entry) = class_info.method_at(*slot) else {
        return; // ditto
    };
    let Some(sig) = m.signature(vtable_entry.sig) else {
        return; // ditto
    };

    if !is_positional(args) {
        return;
    }

    // Check arity
    if args.len() != sig.arity() {
        errors.push(VerifyError::new(
            format!(
                "method call passes {} arguments, signature takes {}",
                args.len(),
                sig.arity()
            ),
            e.span,
        ));
        return;
    }

    // Each argument must be assignable TO its parameter
    for (i, (a, p)) in args.iter().zip(&sig.params).enumerate() {
        if !assignable(m, a.value().ty, *p) {
            errors.push(VerifyError::new(
                format!(
                    "method call: argument {i} is {:?}, parameter is {:?}",
                    a.value().ty,
                    p
                ),
                e.span,
            ));
        }
    }

    // The signature's return type must be assignable TO what the node claims
    if !assignable(m, sig.return_ty, e.ty) {
        errors.push(VerifyError::new(
            format!(
                "method call returns {:?}, node says {:?}",
                sig.return_ty, e.ty
            ),
            e.span,
        ));
    }
}

fn check_condition(
    _m: &TirModule,
    context: &str,
    cond: &TirExpr,
    errors: &mut Vec<VerifyError>,
) {
    // Skip if condition is Dynamic
    if matches!(cond.ty, BackendTy::Dynamic(_)) {
        return;
    }

    if cond.ty != BackendTy::Bool {
        errors.push(VerifyError::new(
            format!("{context} condition must be Bool, node says {:?}", cond.ty),
            cond.span,
        ));
    }
}

fn check_let(
    m: &TirModule,
    declared_ty: BackendTy,
    init: &TirExpr,
    errors: &mut Vec<VerifyError>,
) {
    // The initializer must be assignable TO the declared type
    if !assignable(m, init.ty, declared_ty) {
        errors.push(VerifyError::new(
            format!(
                "let binding declares {:?}, initializer is {:?}",
                declared_ty, init.ty
            ),
            init.span,
        ));
    }
}

fn check_return(
    m: &TirModule,
    f: &TirFunction,
    returned: &TirExpr,
    errors: &mut Vec<VerifyError>,
) {
    // The returned value must be assignable TO the function's return_ty
    if !assignable(m, returned.ty, f.return_ty) {
        errors.push(VerifyError::new(
            format!(
                "function `{}` declares return type {:?}, returned {:?}",
                f.name, f.return_ty, returned.ty
            ),
            returned.span,
        ));
    }
}

fn check_return_none(
    _m: &TirModule,
    f: &TirFunction,
    errors: &mut Vec<VerifyError>,
) {
    // Skip if function return type is Dynamic
    if matches!(f.return_ty, BackendTy::Dynamic(_)) {
        return;
    }

    if f.return_ty != BackendTy::Void {
        errors.push(VerifyError::new(
            format!(
                "function `{}` declares return type {:?}, bare return is Void",
                f.name, f.return_ty
            ),
            crate::node::Span::EMPTY,
        ));
    }
}
