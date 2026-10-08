use super::recorder::Recorder;
use super::Checker;
use varn_core::ast::decorators::{match_builtin, BuiltinDecorator};
use varn_core::ast::Decorator;
use varn_core::diagnostics::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;

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
        rec: &mut Recorder,
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
                    rec,
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
                rec,
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
        rec: &mut Recorder,
        d: &Decorator,
        expected_arity: usize,
        check_return: bool,
        target_kind: &str,
        target_name: &str,
        bind: &BindResult,
    ) {
        self.check_expr(rec, d.expression, bind);
        let deco_ty = self.infer_type(rec, d.expression, bind);
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
                    varn_sem::types::Type::resolved(ft.return_type)
                        .display(&self.ty_table, &bind.interner),
                ),
            )
            .with_range(d.range),
        );
    }

    pub(crate) fn warn_if_deprecated(
        &mut self,
        sid: varn_sem::symbol::SymbolId,
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

    fn is_void_ty(
        table: &varn_sem::types::CheckerTyTable,
        id: varn_sem::types::CheckerTyId,
    ) -> bool {
        matches!(
            table.get(id),
            varn_core::TypeKind::Primitive(varn_core::LangPrimitive::Void)
        )
    }

    fn is_null_ty(
        table: &varn_sem::types::CheckerTyTable,
        id: varn_sem::types::CheckerTyId,
    ) -> bool {
        matches!(
            table.get(id),
            varn_core::TypeKind::Primitive(varn_core::LangPrimitive::Null)
        )
    }

    fn is_dynamic_ty(
        table: &varn_sem::types::CheckerTyTable,
        id: varn_sem::types::CheckerTyId,
    ) -> bool {
        matches!(
            table.get(id),
            varn_core::TypeKind::Primitive(varn_core::LangPrimitive::Dynamic)
        )
    }

    fn is_callable_ty(
        table: &varn_sem::types::CheckerTyTable,
        id: varn_sem::types::CheckerTyId,
    ) -> bool {
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
            TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::TypePredicate { .. } => false,
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
