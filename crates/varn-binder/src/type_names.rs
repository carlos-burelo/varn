use std::sync::Arc;

use varn_core::{Atom, Diagnostic, ErrorCode, SourceRange};

use varn_sem::scope::{ScopeId, ScopeKind};
use varn_sem::symbol::SymbolKind;

impl super::Binder<'_> {
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

    pub(crate) fn note_value_decl(&mut self, name: Atom, sym: &varn_sem::symbol::Symbol) {
        let scope = self.current;
        let in_namespace = match self.scopes.get(scope).kind {
            ScopeKind::Namespace => true,
            ScopeKind::Global | ScopeKind::Module => false,
            ScopeKind::Function | ScopeKind::Block | ScopeKind::Class | ScopeKind::Interface => {
                return
            }
        };
        let is_value = matches!(
            sym.kind,
            SymbolKind::Function | SymbolKind::Var | SymbolKind::Let | SymbolKind::Const
        );
        let foreign =
            sym.origin_module.is_some() || (sym.line == 0 && sym.full_range.start.line == 0);
        if !is_value || foreign {
            return;
        }
        let mut range = sym.full_range;
        if range.start.line == 0 && range.end.line == 0 {
            range.start.line = sym.line;
            range.end.line = sym.line;
        }
        let Some(&(first_scope, first_in_namespace, first)) = self.value_decls.get(&name) else {
            self.value_decls.insert(name, (scope, in_namespace, range));
            return;
        };
        if first_scope == scope || !(in_namespace || first_in_namespace) {
            return;
        }
        let msg = format!(
            "duplicate declaration of '{}': a name declared inside a namespace is module-wide",
            self.interner.resolve(name)
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
