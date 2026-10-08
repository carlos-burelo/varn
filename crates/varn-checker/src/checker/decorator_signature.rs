use super::Checker;
use crate::binder::BindResult;
use varn_core::ast::decorators::{match_builtin, BuiltinDecorator};
use varn_core::ast::Decorator;
use varn_core::diagnostics::{Diagnostic, ErrorCode};

pub(super) enum DecoratorTarget {
    Function,
    Method,
    Class,
    Getter,
    Setter,
    Constructor,
    Property,
}

impl DecoratorTarget {
    fn arity(&self) -> usize {
        match self {
            DecoratorTarget::Function | DecoratorTarget::Class => 1,
            DecoratorTarget::Method
            | DecoratorTarget::Getter
            | DecoratorTarget::Setter
            | DecoratorTarget::Constructor
            | DecoratorTarget::Property => 2,
        }
    }

    fn kind_str(&self) -> &'static str {
        match self {
            DecoratorTarget::Function => "function",
            DecoratorTarget::Method => "method",
            DecoratorTarget::Class => "class",
            DecoratorTarget::Getter => "getter",
            DecoratorTarget::Setter => "setter",
            DecoratorTarget::Constructor => "constructor",
            DecoratorTarget::Property => "property",
        }
    }

    fn check_return(&self) -> bool {
        !matches!(self, DecoratorTarget::Property)
    }
}

impl<'r> Checker<'r> {
    pub(super) fn check_decorator_signatures(
        &mut self,
        decorators: &[Decorator],
        target: DecoratorTarget,
        target_name: &str,
        bind: &BindResult,
    ) {
        let expected_arity = target.arity();
        let target_kind = target.kind_str();
        let check_return = target.check_return();
        let builtins = match_builtin(self.ast_arena, &bind.interner, decorators);
        for (d, b) in decorators.iter().zip(builtins.iter()) {
            if bind.user_decorators.contains(&d.range.start.offset) {
                self.check_user_decorator(
                    d,
                    expected_arity,
                    check_return,
                    target_kind,
                    target_name,
                    bind,
                );
                continue;
            }
            if let Some(kind) = Self::builtin_kind(b) {
                self.check_builtin_placement(&kind, target_kind, target_name, d.range, bind);
                continue;
            }
            if let Some(Err(shape)) = &b.result {
                self.emit(
                    Diagnostic::error(ErrorCode::InvalidDecoratorSignature, shape.to_string())
                        .with_range(d.range),
                );
                continue;
            }
            self.check_user_decorator(
                d,
                expected_arity,
                check_return,
                target_kind,
                target_name,
                bind,
            );
        }
    }

    fn check_user_decorator(
        &mut self,
        d: &Decorator,
        expected_arity: usize,
        check_return: bool,
        target_kind: &str,
        target_name: &str,
        bind: &BindResult,
    ) {
        self.check_expr(d.expression, bind);
        let deco_ty = self.infer_type(d.expression, bind);
        if deco_ty.is_error() {
            return;
        }
        let is_factory_call = matches!(
            self.ast_arena.expr(d.expression).kind,
            varn_core::ast::ExprKind::Call { .. }
        );
        if is_factory_call
            && (Self::is_void_ty(&self.ty_table, deco_ty.0)
                || Self::is_null_ty(&self.ty_table, deco_ty.0))
        {
            return;
        }
        let varn_core::TypeKind::Fn(fid) = self.ty_table.get(deco_ty.0) else {
            if Self::is_dynamic_ty(&self.ty_table, deco_ty.0) {
                return;
            }
            self.emit(
                Diagnostic::error(
                    ErrorCode::InvalidDecoratorSignature,
                    format!(
                        "decorator on {target_kind} '{target_name}' must be a function, got '{}'",
                        deco_ty.display(&self.ty_table, &bind.interner),
                    ),
                )
                .with_range(d.range),
            );
            return;
        };
        let ft = self.ty_table.get_function(fid).clone();
        let required = ft
            .params
            .iter()
            .filter(|p| !p.optional && !p.is_rest)
            .count();
        let has_rest = ft.params.iter().any(|p| p.is_rest);
        if expected_arity < required || (!has_rest && expected_arity > ft.params.len()) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::InvalidDecoratorSignature,
                    format!(
                        "decorator on {target_kind} '{target_name}' takes {} argument(s), expected {expected_arity}",
                        ft.params.len(),
                    ),
                )
                .with_range(d.range),
            );
            return;
        }
        if Self::is_void_ty(&self.ty_table, ft.return_type)
            || Self::is_null_ty(&self.ty_table, ft.return_type)
            || Self::is_dynamic_ty(&self.ty_table, ft.return_type)
        {
            return;
        }
        if !check_return || Self::is_callable_ty(&self.ty_table, ft.return_type) {
            return;
        }
        self.emit(
            Diagnostic::error(
                ErrorCode::InvalidDecoratorSignature,
                format!(
                    "decorator on {target_kind} '{target_name}' returns '{}' which cannot replace the decorated value (return void, null, a function or a compatible value instead)",
                    crate::types::Type::resolved(ft.return_type)
                        .display(&self.ty_table, &bind.interner),
                ),
            )
            .with_range(d.range),
        );
    }

    pub(crate) fn warn_if_deprecated(
        &mut self,
        sid: crate::symbol::SymbolId,
        name: &str,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        let Some(msg) = bind.arena.get(sid).deprecated.clone() else {
            return;
        };
        if !self.warned_deprecated.insert((sid, range.start.offset)) {
            return;
        }
        let text = if msg.is_empty() {
            format!("'{name}' is deprecated")
        } else {
            format!("'{name}' is deprecated: {msg}")
        };
        self.emit(Diagnostic::warning(ErrorCode::DeprecatedUse, text).with_range(range));
    }

    fn builtin_kind(b: &varn_core::ast::decorators::BuiltinMatch) -> Option<BuiltinKind> {
        match &b.result {
            Some(Ok(BuiltinDecorator::Deprecated { .. })) => Some(BuiltinKind::Deprecated),
            Some(Ok(BuiltinDecorator::Pure)) => Some(BuiltinKind::Pure),
            Some(Ok(BuiltinDecorator::Inline)) => Some(BuiltinKind::Inline),
            Some(Ok(BuiltinDecorator::Capability { .. })) => Some(BuiltinKind::Capability),
            Some(Ok(BuiltinDecorator::Test)) => Some(BuiltinKind::Test),
            Some(Err(_)) | None => None,
        }
    }

    fn check_builtin_placement(
        &mut self,
        kind: &BuiltinKind,
        target_kind: &str,
        target_name: &str,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        let _ = bind;
        let ok = match kind {
            BuiltinKind::Deprecated => true,
            BuiltinKind::Pure | BuiltinKind::Capability => {
                matches!(target_kind, "function" | "method" | "getter" | "setter")
            }
            BuiltinKind::Inline | BuiltinKind::Test => target_kind == "function",
        };
        if !ok {
            self.emit(
                Diagnostic::error(
                    ErrorCode::InvalidDecoratorTarget,
                    format!(
                        "`@{}` is only supported on functions, not on {target_kind} '{target_name}'",
                        kind.name(),
                    ),
                )
                .with_range(range),
            );
        }
    }

    fn is_void_ty(table: &crate::types::CheckerTyTable, id: crate::types::CheckerTyId) -> bool {
        matches!(
            table.get(id),
            varn_core::TypeKind::Primitive(varn_core::LangPrimitive::Void)
        )
    }

    fn is_null_ty(table: &crate::types::CheckerTyTable, id: crate::types::CheckerTyId) -> bool {
        matches!(
            table.get(id),
            varn_core::TypeKind::Primitive(varn_core::LangPrimitive::Null)
        )
    }

    fn is_dynamic_ty(table: &crate::types::CheckerTyTable, id: crate::types::CheckerTyId) -> bool {
        matches!(
            table.get(id),
            varn_core::TypeKind::Primitive(varn_core::LangPrimitive::Dynamic)
        )
    }

    fn is_callable_ty(table: &crate::types::CheckerTyTable, id: crate::types::CheckerTyId) -> bool {
        use varn_core::TypeKind;
        match table.get(id) {
            TypeKind::Fn(_)
            | TypeKind::Named(_, _)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Object(_)
            | TypeKind::Builtin(_)
            | TypeKind::EnumVariant { .. } => true,
            TypeKind::Primitive(p) => matches!(
                p,
                varn_core::LangPrimitive::Dynamic
                    | varn_core::LangPrimitive::Void
                    | varn_core::LangPrimitive::Null
            ),
            _ => false,
        }
    }
}

enum BuiltinKind {
    Deprecated,
    Pure,
    Inline,
    Capability,
    Test,
}

impl BuiltinKind {
    fn name(&self) -> &'static str {
        match self {
            BuiltinKind::Deprecated => "deprecated",
            BuiltinKind::Pure => "pure",
            BuiltinKind::Inline => "inline",
            BuiltinKind::Capability => "capability",
            BuiltinKind::Test => "test",
        }
    }
}

pub(super) fn is_pure_fn(
    decorators: &[Decorator],
    arena: &varn_core::ast::AstArena,
    bind: &BindResult,
) -> bool {
    match_builtin(arena, &bind.interner, decorators)
        .into_iter()
        .zip(decorators.iter())
        .any(|(m, d)| {
            !bind.user_decorators.contains(&d.range.start.offset)
                && matches!(m.result, Some(Ok(BuiltinDecorator::Pure)))
        })
}

pub(super) fn caps_of(
    decorators: &[Decorator],
    arena: &varn_core::ast::AstArena,
    bind: &BindResult,
) -> Vec<String> {
    let mut out = Vec::new();
    for (m, d) in match_builtin(arena, &bind.interner, decorators)
        .into_iter()
        .zip(decorators.iter())
    {
        if bind.user_decorators.contains(&d.range.start.offset) {
            continue;
        }
        if let Some(Ok(BuiltinDecorator::Capability { domains })) = m.result {
            for d in domains {
                if !out.contains(&d) {
                    out.push(d);
                }
            }
        }
    }
    out
}

impl<'r> Checker<'r> {
    pub(crate) fn forbid_pure(&mut self, what: &str, range: varn_core::SourceRange) {
        self.emit(
            Diagnostic::error(
                ErrorCode::ImpureOperation,
                format!("`@pure` function cannot {what}"),
            )
            .with_range(range),
        );
    }

    pub(crate) fn pure_assign_target_ok(
        &mut self,
        name: varn_core::Atom,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        let Some(pure) = self.pure_scope else {
            return;
        };
        let mut scope = self.current_scope;
        let declarant = loop {
            let sc = bind.scopes.get(scope);
            if sc.lookup(name).is_some() {
                break scope;
            }
            match sc.parent {
                Some(p) => scope = p,
                None => {
                    self.forbid_pure(
                        "assign to non-local state (only parameters and function locals can be assigned)",
                        range,
                    );
                    return;
                }
            }
        };
        let mut scope = declarant;
        let sid = bind.scopes.get(declarant).lookup(name).unwrap();
        loop {
            if scope == pure {
                if matches!(
                    bind.arena.get(sid).kind,
                    crate::symbol::SymbolKind::Parameter
                        | crate::symbol::SymbolKind::Let
                        | crate::symbol::SymbolKind::Var
                        | crate::symbol::SymbolKind::Const
                ) {
                    return;
                }
                break;
            }
            let sc = bind.scopes.get(scope);
            if sc.kind == crate::scope::ScopeKind::Function {
                break;
            }
            match sc.parent {
                Some(p) => scope = p,
                None => break,
            }
        }
        self.forbid_pure(
            "assign to non-local state (only parameters and function locals can be assigned)",
            range,
        );
    }

    pub(crate) fn check_pure_callee(
        &mut self,
        sid: Option<usize>,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        if self.pure_scope.is_none() {
            return;
        }
        let Some(sid) = sid else { return };
        if sid >= bind.arena.len() {
            return;
        }
        let sym = bind.arena.get(sid);
        if sym.is_pure {
            return;
        }
        if !matches!(
            sym.kind,
            crate::symbol::SymbolKind::Function | crate::symbol::SymbolKind::Method
        ) {
            return;
        }
        self.forbid_pure(
            &format!(
                "call '{}' which is not marked `@pure`",
                bind.interner.resolve(sym.name)
            ),
            range,
        );
    }

    pub(crate) fn check_capability_callee(
        &mut self,
        sid: Option<usize>,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        let Some(granted) = self.enclosing_caps.as_ref() else {
            return;
        };
        let Some(sid) = sid else { return };
        if sid >= bind.arena.len() {
            return;
        }
        let sym = bind.arena.get(sid);
        if sym.capabilities.is_empty() {
            return;
        }
        let missing: Vec<&String> = sym
            .capabilities
            .iter()
            .filter(|c| !granted.contains(c))
            .collect();
        if missing.is_empty() {
            return;
        }
        self.emit(
            Diagnostic::error(
                ErrorCode::CapabilityViolation,
                format!(
                    "calling '{}' requires capability '{}' (declare it with `@capability` to propagate)",
                    bind.interner.resolve(sym.name),
                    missing[0],
                ),
            )
            .with_range(range),
        );
    }
}
