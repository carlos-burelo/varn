use super::TokenStream;
use vn_core::ast::{
    Arg, Decl, ExprKind, Pattern, StmtId, StmtKind, VarDeclarator, VarKind, VariableDecl,
};
use vn_core::{Atom, SourceRange};

pub(super) struct RegexLiteral {
    binding: Atom,
    pattern: String,
    flags: String,
    range: SourceRange,
}

impl TokenStream {
    pub fn regex_binding(&mut self, pattern: &str, flags: &str, range: SourceRange) -> Atom {
        if let Some(known) = self
            .regex_literals
            .iter()
            .find(|l| l.pattern == pattern && l.flags == flags)
        {
            return known.binding;
        }
        let binding = self
            .interner
            .intern(&vn_core::intrinsics::regex_literal_binding(
                self.regex_literals.len(),
            ));
        self.regex_literals.push(RegexLiteral {
            binding,
            pattern: pattern.to_owned(),
            flags: flags.to_owned(),
            range,
        });
        binding
    }

    pub fn hoist_regex_literals(&mut self) -> Vec<StmtId> {
        let literals = std::mem::take(&mut self.regex_literals);
        if literals.is_empty() {
            return Vec::new();
        }
        let class = self.interner.intern(vn_core::regex_pattern::CLASS);
        let mut hoisted = Vec::new();
        for literal in literals {
            let range = literal.range;
            let callee = self.expr(range, ExprKind::Identifier { name: class });
            let pattern = self.expr(
                range,
                ExprKind::StrLiteral {
                    value: literal.pattern,
                },
            );
            let flags = self.expr(
                range,
                ExprKind::StrLiteral {
                    value: literal.flags,
                },
            );
            let init = self.expr(
                range,
                ExprKind::New {
                    callee,
                    type_args: Vec::new(),
                    args: vec![Arg::Positional(pattern), Arg::Positional(flags)],
                },
            );
            let decl = VariableDecl {
                kind: VarKind::Const,
                ast_id: self.next_ast_id(),
                declarators: vec![VarDeclarator {
                    id: Pattern::Identifier {
                        name: literal.binding,
                        range,
                    },
                    type_ann: None,
                    init: Some(init),
                    range,
                }],
                is_declare: false,
                doc: None,
                range,
            };
            hoisted.push(self.stmt(range, StmtKind::Decl(Box::new(Decl::Variable(decl)))));
        }
        hoisted
    }
}
