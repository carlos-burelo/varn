use std::sync::Arc;

use varn_core::{Atom, Diagnostic, ErrorCode, SourceRange};

use varn_sem::scope::{ScopeId, ScopeKind};
use varn_sem::symbol::SymbolKind;

impl super::Binder<'_> {
    pub(crate) fn declared_type_name(
        &mut self,
        id: Atom,
        ast_id: varn_core::ast::AstId,
    ) -> Arc<str> {
        let bare = self.interner.resolve(id);
        let name: Arc<str> = match self.namespace_path.last() {
            Some(outer) => Arc::from(format!("{outer}::{bare}")),
            None => Arc::from(bare),
        };
        self.intern_local(&name);
        self.declared_type_names.insert(ast_id, name.clone());
        name
    }

    pub(crate) fn type_name_of(&self, id: Atom, ast_id: varn_core::ast::AstId) -> Arc<str> {
        self.declared_type_names
            .get(&ast_id)
            .cloned()
            .unwrap_or_else(|| Arc::from(self.interner.resolve(id)))
    }

    pub(crate) fn declare_kind(&mut self, name: Arc<str>, kind: varn_sem::bind::DeclKind) {
        self.decl_kinds
            .entry(name)
            .and_modify(|held| *held = (*held).min(kind))
            .or_insert(kind);
    }

    pub(crate) fn note_type_decl(&mut self, name: &Arc<str>, scope: ScopeId, range: SourceRange) {
        let Some(&(first_scope, first)) = self.type_decls.get(name) else {
            self.type_decls.insert(name.clone(), (scope, range));
            return;
        };
        if first_scope == scope || first.start.offset == range.start.offset {
            return;
        }
        let msg = format!(
            "duplicate declaration of '{name}': class and enum names are module-wide, \
             even when declared inside a function"
        );
        let diag = Diagnostic::error(ErrorCode::DuplicateDeclaration, msg)
            .with_file(self.source_file.clone())
            .with_range(range)
            .with_related("original declaration here", self.source_file.clone(), first);
        self.emit(diag);
    }

    pub(crate) fn check_local_class_capture(&mut self, name: Atom, range: SourceRange) {
        let mut scope = self.current;
        let mut crossed_class = false;
        loop {
            let s = self.scopes.get(scope);
            if let Some(id) = s.lookup(name) {
                let in_function = matches!(s.kind, ScopeKind::Function | ScopeKind::Block);
                let kind = self.arena.get(id).kind;
                let is_value = matches!(
                    kind,
                    SymbolKind::Var
                        | SymbolKind::Let
                        | SymbolKind::Const
                        | SymbolKind::Parameter
                        | SymbolKind::Function
                        | SymbolKind::Namespace
                );
                if crossed_class && in_function && is_value {
                    let msg = format!(
                        "'{}' belongs to the function this class is declared in: a class \
                         or enum declared inside a function is one module-wide type, so its \
                         members cannot use that function's locals",
                        self.interner.resolve(name)
                    );
                    let diag = Diagnostic::error(ErrorCode::UnknownSymbol, msg)
                        .with_file(self.source_file.clone())
                        .with_range(range);
                    self.emit(diag);
                }
                return;
            }
            if s.kind == ScopeKind::Class {
                crossed_class = true;
            }
            match s.parent {
                Some(p) => scope = p,
                None => return,
            }
        }
    }
}
