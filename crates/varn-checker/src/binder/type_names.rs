//! Class and enum names are module-wide.
//!
//! A class or enum declared inside a function is still one nominal type of the
//! module: the type tables, the backend's class definitions and the runtime
//! global holding its class object are all keyed by its name. So two such
//! declarations of one name — in two functions, or a function and the top
//! level — are one name declared twice, even though neither scope sees the
//! other. The scope-local duplicate check cannot see that; this does.
//!
//! For the same reason such a class cannot use the values of the function it
//! is declared in: its methods belong to the one module-wide class, built once,
//! not to a call of that function, so "that call's `x`" has no meaning inside
//! them. A reference that reaches a local, parameter or local function across
//! the class boundary is reported; types stay visible, being module-wide.

use std::sync::Arc;

use varn_core::{Atom, Diagnostic, ErrorCode, SourceRange};

use crate::scope::{ScopeId, ScopeKind};
use crate::symbol::SymbolKind;

impl super::Binder<'_> {
    /// Record that `name` is declared as a class or enum at `range`, in
    /// `scope`. A same-named type from another scope is reported; one from the
    /// same scope is the ordinary duplicate the scope check reports, and one at
    /// the same position is this declaration bound again.
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

    /// Report `name`, used at `range`, when it resolves across a class
    /// boundary to a value of an enclosing function (see the module docs).
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
