use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::{Decl, ExportDecl, ExprId, ExprKind, VarKind};
use varn_core::{Diagnostic, ErrorCode, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::symbol::SymbolKind;

impl<'r> Checker<'r> {
    pub(super) fn require_namespace_member_use(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        bind: &BindResult,
    ) {
        if self.namespace_bases.remove(&expr) || !self.denotes_namespace(rec, expr, bind) {
            return;
        }
        self.emit(
            Diagnostic::error(
                ErrorCode::NamespaceAsValue,
                "a namespace is a scope, not a value: name one of its members",
            )
            .with_range(self.ast_arena.expr(expr).range),
        );
    }

    pub(super) fn denotes_namespace(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        bind: &BindResult,
    ) -> bool {
        use varn_sem::types::TypeContext;
        if let ExprKind::Identifier { name } = &self.ast_arena.expr(expr).kind {
            let scope = bind.scopes.get(self.current_scope);
            return scope
                .resolve(*name, &bind.scopes)
                .is_some_and(|id| bind.arena.get(id).kind == SymbolKind::Namespace);
        }
        let ExprKind::Member {
            object,
            computed: false,
            ..
        } = &self.ast_arena.expr(expr).kind
        else {
            return false;
        };
        let object = *object;
        if !self.denotes_namespace(rec, object, bind) {
            return false;
        }
        let ty = self.infer_type(rec, expr, bind);
        let TypeKind::Named(name, origin) = self.ty_table.get(ty.0) else {
            return false;
        };
        let name = self.resolve_bind_atom(bind, name);
        let origin = origin.map(|o| self.resolve_bind_atom(bind, o));
        let view = varn_sem::bind::BindView::new(bind, self.resolver);
        view.get_namespace_members(&name, origin.as_deref())
            .is_some()
            && view.get_class_members(&name, origin.as_deref()).is_none()
            && view.get_enum_members(&name, origin.as_deref()).is_none()
    }

    pub(crate) fn reject_mutable_export(&mut self, export: &ExportDecl, bind: &BindResult) {
        let message = "an exported binding is constant: export a 'const', \
                       and change shared state through a function";
        match export {
            ExportDecl::Decl { declaration, .. } => {
                if let Decl::Variable(v) = declaration.as_ref()
                    && v.kind == VarKind::Let
                {
                    self.emit(
                        Diagnostic::error(ErrorCode::MutableExport, message).with_range(v.range),
                    );
                }
            }
            ExportDecl::Named {
                specifiers,
                source: None,
                ..
            } => {
                let scope = bind.scopes.get(self.current_scope);
                let mutable: Vec<_> = specifiers
                    .iter()
                    .filter(|spec| {
                        scope.resolve(spec.local, &bind.scopes).is_some_and(|id| {
                            let sym = bind.arena.get(id);
                            sym.origin_module.is_none()
                                && matches!(sym.kind, SymbolKind::Let | SymbolKind::Var)
                        })
                    })
                    .map(|spec| spec.range)
                    .collect();
                for range in mutable {
                    self.emit(
                        Diagnostic::error(ErrorCode::MutableExport, message).with_range(range),
                    );
                }
            }
            ExportDecl::Named { .. } | ExportDecl::All { .. } | ExportDecl::Default { .. } => {}
        }
    }
}
