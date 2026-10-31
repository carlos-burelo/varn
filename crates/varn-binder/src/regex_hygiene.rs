use super::Binder;
use varn_core::ast::{Decl, Pattern, Program, StmtKind};
use varn_core::{Diagnostic, ErrorCode};

impl Binder<'_> {
    pub(super) fn check_regex_literal_hygiene(&mut self, program: &Program) {
        let class = self.interner.intern(varn_core::regex_pattern::CLASS);
        let Some(declared) = self.scopes.get(self.current).lookup(class) else {
            return;
        };
        let language_module = self.interner.intern(varn_core::regex_pattern::MODULE);
        if self.arena.get(declared).origin_module == Some(language_module) {
            return;
        }
        for &stmt in &program.body {
            let StmtKind::Decl(decl) = &self.ast_arena.stmt(stmt).kind else {
                continue;
            };
            let Decl::Variable(variable) = decl.as_ref() else {
                continue;
            };
            let is_literal = variable.declarators.iter().any(|d| match &d.id {
                Pattern::Identifier { name, .. } => {
                    varn_core::intrinsics::is_regex_literal_binding(self.interner.resolve(*name))
                }
                Pattern::Array { .. }
                | Pattern::Object { .. }
                | Pattern::Assignment { .. }
                | Pattern::Rest { .. } => false,
            });
            if is_literal {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::InvalidRegex,
                        "this module declares its own `Regex`, so a /regex/ literal would build that class instead of the language's Regex; rename the declaration or use `new` on the language's type through another module".to_owned(),
                    )
                    .with_range(variable.range),
                );
                return;
            }
        }
    }
}
