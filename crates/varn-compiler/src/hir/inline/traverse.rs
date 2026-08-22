use crate::hir::{
    HirArrayEl, HirAssignTarget, HirExpr, HirObjectProp, HirOptionalProperty, HirPropKey, HirStmt,
    HirTemplatePart,
};

/// Immutable walk over an expression tree (does not cross into nested
/// `HirFunction` bodies — validated candidate bodies contain none).
pub(crate) fn walk_exprs<'a>(e: &'a HirExpr, f: &mut impl FnMut(&'a HirExpr)) {
    f(e);
    // Clone-free traversal via the mutable-children helper is not possible on
    // a shared reference; mirror the child list instead.
    use HirExpr::*;
    match e {
        Int(_)
        | Float(_)
        | Str(_)
        | Bool(_)
        | Char(_)
        | Decimal(_)
        | BigInt(_)
        | Null
        | Regex { .. }
        | Var(_)
        | This
        | Super
        | SuperMember { .. } => {}
        NonNull(x)
        | TryOp(x)
        | Spread(x)
        | Await(x)
        | Spawn(x)
        | Yield(x)
        | TypeTest { value: x, .. } => walk_exprs(x, f),
        Sequence(xs)
        | SelfCall { args: xs, .. }
        | SuperCall { args: xs }
        | SuperMethodCall { args: xs, .. } => {
            for x in xs {
                walk_exprs(x, f);
            }
        }
        Range { start, end, .. } => {
            walk_exprs(start, f);
            walk_exprs(end, f);
        }
        Template(parts) => {
            for p in parts {
                if let HirTemplatePart::Expr(x) = p {
                    walk_exprs(x, f);
                }
            }
        }
        Assign { value, .. } => walk_exprs(value, f),
        Update { .. } => {}
        Binary { lhs, rhs, .. } | Logical { lhs, rhs, .. } => {
            walk_exprs(lhs, f);
            walk_exprs(rhs, f);
        }
        Unary { operand, .. } => walk_exprs(operand, f),
        Call { callee, args, .. } => {
            walk_exprs(callee, f);
            for x in args {
                walk_exprs(x, f);
            }
        }
        Member { object, .. }
        | MemberMaybe { object, .. }
        | GetFixedField { object, .. }
        | ModuleSlot { object, .. }
        | ObjectRest { object, .. } => walk_exprs(object, f),
        Index { object, index, .. } => {
            walk_exprs(object, f);
            walk_exprs(index, f);
        }
        MethodCall { recv, args, .. } | ExtensionCall { recv, args, .. } => {
            walk_exprs(recv, f);
            for x in args {
                walk_exprs(x, f);
            }
        }
        NativeMethodCall { object, args, .. } | IntrinsicCall { object, args, .. } => {
            walk_exprs(object, f);
            for x in args {
                walk_exprs(x, f);
            }
        }
        Conditional { test, cons, alt } => {
            walk_exprs(test, f);
            walk_exprs(cons, f);
            walk_exprs(alt, f);
        }
        Array(els) | Tuple(els) => {
            for el in els {
                if let HirArrayEl::Expr(x) | HirArrayEl::Spread(x) = el {
                    walk_exprs(x, f);
                }
            }
        }
        Object { properties } | Record { properties } => {
            for p in properties {
                match p {
                    HirObjectProp::Property { key, value } => {
                        if let HirPropKey::Computed(x) = key {
                            walk_exprs(x, f);
                        }
                        walk_exprs(value, f);
                    }
                    HirObjectProp::Spread(x) => walk_exprs(x, f),
                    HirObjectProp::Method { .. } => {}
                }
            }
        }
        OptionalChain { object, property } => {
            walk_exprs(object, f);
            match property {
                HirOptionalProperty::Member(_)
                | HirOptionalProperty::ModuleSlot(_)
                | HirOptionalProperty::Extension(_) => {}
                HirOptionalProperty::Index(x) => walk_exprs(x, f),
                HirOptionalProperty::Call(args)
                | HirOptionalProperty::MethodCall(_, args)
                | HirOptionalProperty::ExtensionCall(_, args) => {
                    for x in args {
                        walk_exprs(x, f);
                    }
                }
            }
        }
        TaggedTemplate { tag, template } => {
            walk_exprs(tag, f);
            walk_exprs(template, f);
        }
        Closure { .. } | Class(_) | Enum(_) | Match { .. } => {}
    }
}

/// Apply `f` to every direct child expression of `e`. Does not descend into
/// nested `HirFunction` bodies (closures, classes, enums, object methods).
pub(crate) fn for_each_child_expr_mut(e: &mut HirExpr, f: &mut impl FnMut(&mut HirExpr)) {
    use HirExpr::*;
    match e {
        Int(_)
        | Float(_)
        | Str(_)
        | Bool(_)
        | Char(_)
        | Decimal(_)
        | BigInt(_)
        | Null
        | Regex { .. }
        | Var(_)
        | This
        | Super
        | SuperMember { .. } => {}
        NonNull(x)
        | TryOp(x)
        | Spread(x)
        | Await(x)
        | Spawn(x)
        | Yield(x)
        | TypeTest { value: x, .. } => f(x),
        Sequence(xs)
        | SelfCall { args: xs, .. }
        | SuperCall { args: xs }
        | SuperMethodCall { args: xs, .. } => {
            for x in xs {
                f(x);
            }
        }
        Range { start, end, .. } => {
            f(start);
            f(end);
        }
        Template(parts) => {
            for p in parts {
                if let HirTemplatePart::Expr(x) = p {
                    f(x);
                }
            }
        }
        Assign { target, value } => {
            for_each_assign_target_expr_mut(target, f);
            f(value);
        }
        Update { target, .. } => for_each_assign_target_expr_mut(target, f),
        Binary { lhs, rhs, .. } | Logical { lhs, rhs, .. } => {
            f(lhs);
            f(rhs);
        }
        Unary { operand, .. } => f(operand),
        Call { callee, args, .. } => {
            f(callee);
            for x in args {
                f(x);
            }
        }
        Member { object, .. }
        | MemberMaybe { object, .. }
        | GetFixedField { object, .. }
        | ModuleSlot { object, .. }
        | ObjectRest { object, .. } => f(object),
        Index { object, index, .. } => {
            f(object);
            f(index);
        }
        MethodCall { recv, args, .. } | ExtensionCall { recv, args, .. } => {
            f(recv);
            for x in args {
                f(x);
            }
        }
        NativeMethodCall { object, args, .. } | IntrinsicCall { object, args, .. } => {
            f(object);
            for x in args {
                f(x);
            }
        }
        Conditional { test, cons, alt } => {
            f(test);
            f(cons);
            f(alt);
        }
        Array(els) | Tuple(els) => {
            for el in els {
                if let HirArrayEl::Expr(x) | HirArrayEl::Spread(x) = el {
                    f(x);
                }
            }
        }
        Object { properties } | Record { properties } => {
            for p in properties {
                match p {
                    HirObjectProp::Property { key, value } => {
                        if let HirPropKey::Computed(x) = key {
                            f(x);
                        }
                        f(value);
                    }
                    HirObjectProp::Spread(x) => f(x),
                    HirObjectProp::Method { .. } => {}
                }
            }
        }
        OptionalChain { object, property } => {
            f(object);
            match property {
                HirOptionalProperty::Member(_)
                | HirOptionalProperty::ModuleSlot(_)
                | HirOptionalProperty::Extension(_) => {}
                HirOptionalProperty::Index(x) => f(x),
                HirOptionalProperty::Call(args)
                | HirOptionalProperty::MethodCall(_, args)
                | HirOptionalProperty::ExtensionCall(_, args) => {
                    for x in args {
                        f(x);
                    }
                }
            }
        }
        TaggedTemplate { tag, template } => {
            f(tag);
            f(template);
        }
        Match { subject, cases } => {
            f(subject);
            for c in cases {
                if let Some(g) = &mut c.guard {
                    f(g);
                }
                if let Some(r) = &mut c.result {
                    f(r);
                }
            }
        }
        Closure { .. } | Class(_) | Enum(_) => {}
    }
}

pub(crate) fn for_each_assign_target_expr_mut(
    t: &mut HirAssignTarget,
    f: &mut impl FnMut(&mut HirExpr),
) {
    match t {
        HirAssignTarget::Var(_)
        | HirAssignTarget::ModuleSlot { .. }
        | HirAssignTarget::SuperMember { .. } => {}
        HirAssignTarget::Member { object, .. } | HirAssignTarget::SetFixedField { object, .. } => {
            f(object)
        }
        HirAssignTarget::Index { object, index, .. } => {
            f(object);
            f(index);
        }
        HirAssignTarget::SuperIndex { index } => f(index),
    }
}

/// Apply `f` to every expression directly owned by `s` (not those in child
/// statements).
pub(crate) fn for_each_stmt_expr_mut(s: &mut HirStmt, f: &mut impl FnMut(&mut HirExpr)) {
    match s {
        HirStmt::Expr(e)
        | HirStmt::Let { value: e, .. }
        | HirStmt::Assign { value: e, .. }
        | HirStmt::Return(Some(e))
        | HirStmt::Throw(e)
        | HirStmt::ExportDefaultExpr { value: e, .. }
        | HirStmt::While { test: e, .. }
        | HirStmt::DoWhile { test: e, .. }
        | HirStmt::ForClassic { test: e, .. }
        | HirStmt::ForOf { iterable: e, .. }
        | HirStmt::ForIn { object: e, .. }
        | HirStmt::If { test: e, .. }
        | HirStmt::Switch { disc: e, .. } => f(e),
        HirStmt::SetMember { object, value, .. } | HirStmt::SetFixedField { object, value, .. } => {
            f(object);
            f(value);
        }
        HirStmt::SetIndex {
            object,
            index,
            value,
            ..
        } => {
            f(object);
            f(index);
            f(value);
        }
        HirStmt::Return(None)
        | HirStmt::Break
        | HirStmt::Continue
        | HirStmt::CloseUpvalues(_)
        | HirStmt::Import { .. }
        | HirStmt::StoreExport { .. }
        | HirStmt::ExportNamed { .. }
        | HirStmt::ExportAll { .. }
        | HirStmt::Try { .. }
        | HirStmt::Dispose { .. } => {}
    }
    if let HirStmt::Switch { cases, .. } = s {
        for c in cases {
            if let Some(t) = &mut c.test {
                f(t);
            }
        }
    }
}

/// Immutable variant used by the global-mutation scan.
pub(crate) fn for_each_stmt_expr<'a>(s: &'a HirStmt, f: &mut impl FnMut(&'a HirExpr)) {
    match s {
        HirStmt::Expr(e)
        | HirStmt::Let { value: e, .. }
        | HirStmt::Assign { value: e, .. }
        | HirStmt::Return(Some(e))
        | HirStmt::Throw(e)
        | HirStmt::ExportDefaultExpr { value: e, .. }
        | HirStmt::While { test: e, .. }
        | HirStmt::DoWhile { test: e, .. }
        | HirStmt::ForClassic { test: e, .. }
        | HirStmt::ForOf { iterable: e, .. }
        | HirStmt::ForIn { object: e, .. }
        | HirStmt::If { test: e, .. }
        | HirStmt::Switch { disc: e, .. } => f(e),
        HirStmt::SetMember { object, value, .. } | HirStmt::SetFixedField { object, value, .. } => {
            f(object);
            f(value);
        }
        HirStmt::SetIndex {
            object,
            index,
            value,
            ..
        } => {
            f(object);
            f(index);
            f(value);
        }
        HirStmt::Return(None)
        | HirStmt::Break
        | HirStmt::Continue
        | HirStmt::CloseUpvalues(_)
        | HirStmt::Import { .. }
        | HirStmt::StoreExport { .. }
        | HirStmt::ExportNamed { .. }
        | HirStmt::ExportAll { .. }
        | HirStmt::Try { .. }
        | HirStmt::Dispose { .. } => {}
    }
    if let HirStmt::Switch { cases, .. } = s {
        for c in cases {
            if let Some(t) = &c.test {
                f(t);
            }
        }
    }
}

pub(crate) fn child_stmts_mut(s: &mut HirStmt) -> Vec<&mut HirStmt> {
    match s {
        HirStmt::If {
            then_body,
            else_body,
            ..
        } => then_body.iter_mut().chain(else_body.iter_mut()).collect(),
        HirStmt::While { body, .. }
        | HirStmt::ForOf { body, .. }
        | HirStmt::ForIn { body, .. }
        | HirStmt::DoWhile { body, .. } => body.iter_mut().collect(),
        HirStmt::ForClassic { update, body, .. } => {
            update.iter_mut().chain(body.iter_mut()).collect()
        }
        HirStmt::Switch { cases, .. } => cases.iter_mut().flat_map(|c| c.body.iter_mut()).collect(),
        HirStmt::Try {
            block,
            catch,
            finally,
        } => {
            let mut v: Vec<&mut HirStmt> = block.iter_mut().collect();
            if let Some(c) = catch {
                v.extend(c.body.iter_mut());
            }
            if let Some(fin) = finally {
                v.extend(fin.iter_mut());
            }
            v
        }
        _ => Vec::new(),
    }
}

pub(crate) fn push_child_stmts<'a>(s: &'a HirStmt, out: &mut Vec<&'a HirStmt>) {
    match s {
        HirStmt::If {
            then_body,
            else_body,
            ..
        } => {
            out.extend(then_body.iter());
            out.extend(else_body.iter());
        }
        HirStmt::While { body, .. }
        | HirStmt::ForOf { body, .. }
        | HirStmt::ForIn { body, .. }
        | HirStmt::DoWhile { body, .. } => out.extend(body.iter()),
        HirStmt::ForClassic { update, body, .. } => {
            out.extend(update.iter());
            out.extend(body.iter());
        }
        HirStmt::Switch { cases, .. } => {
            for c in cases {
                out.extend(c.body.iter());
            }
        }
        HirStmt::Try {
            block,
            catch,
            finally,
        } => {
            out.extend(block.iter());
            if let Some(c) = catch {
                out.extend(c.body.iter());
            }
            if let Some(fin) = finally {
                out.extend(fin.iter());
            }
        }
        _ => {}
    }
}
