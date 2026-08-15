use varn_core::ast::pattern::MatchPattern;

use varn_core::ast::expr::MatchBody;
use varn_core::ast::{Expr, StmtKind};

use super::*;

impl<'a> Lowerer<'a> {
    pub(super) fn lower_match(
        &mut self,
        subject: &Expr,
        cases: &[varn_core::ast::expr::MatchCase],
        scope: &mut Scope,
    ) -> R<HirExpr> {
        let subject = Box::new(self.lower_expr(subject, scope)?);
        let mut hcases = Vec::with_capacity(cases.len());
        for case in cases {
            scope.push_block();
            let test_res = self.lower_case_test(&case.pattern, scope);
            let test = match test_res {
                Ok(t) => t,
                Err(e) => {
                    scope.pop_block();
                    return Err(e);
                }
            };

            let guard = match &case.guard {
                Some(g) => Some(self.lower_expr(g, scope)?),
                None => None,
            };
            let mut body = Vec::new();
            let result = match &case.body {
                MatchBody::Block(s) => {
                    match &s.kind {
                        StmtKind::Block { stmts } => {
                            for st in stmts {
                                self.lower_stmt(st, scope, &mut body)?;
                            }
                        }
                        _ => self.lower_stmt(s, scope, &mut body)?,
                    }
                    None
                }
                MatchBody::Expr(e) => Some(self.lower_expr(e, scope)?),
            };
            let (captured, disposables) = scope.pop_block();
            block_epilogue(&mut body, captured, disposables);
            hcases.push(HirMatchCase {
                test,
                guard,
                body,
                result,
            });
        }
        Ok(HirExpr::Match {
            subject,
            cases: hcases,
        })
    }

    fn lower_case_test(&mut self, pat: &MatchPattern, scope: &mut Scope) -> R<HirCaseTest> {
        Ok(match pat {
            MatchPattern::Wildcard => HirCaseTest::Wildcard,
            MatchPattern::Literal(lit) => HirCaseTest::Literal(self.lower_expr(lit, scope)?),
            MatchPattern::Identifier(name) => HirCaseTest::Bind(scope.alloc_local(name.clone())),
            MatchPattern::Record { fields, .. } => {
                let mut binds = Vec::with_capacity(fields.len());
                for (field_name, sub_pat) in fields {
                    let binding = match sub_pat {
                        Some(MatchPattern::Identifier(n)) => n.clone(),
                        _ => field_name.clone(),
                    };
                    if &*binding == "_" {
                        binds.push((field_name.clone(), None));
                    } else {
                        binds.push((field_name.clone(), Some(scope.alloc_local(binding))));
                    }
                }
                HirCaseTest::Record { fields: binds }
            }
            MatchPattern::EnumVariant {
                variant_name,
                bindings,
                ..
            } => {
                let mut binds = Vec::with_capacity(bindings.len());
                for b in bindings {
                    if &*b.name == "_" {
                        binds.push(None);
                    } else {
                        binds.push(Some(scope.alloc_local(b.name.clone())));
                    }
                }
                HirCaseTest::EnumVariant {
                    name: variant_name.clone(),
                    binds,
                }
            }

            MatchPattern::Sequence(_) | MatchPattern::Type { .. } => HirCaseTest::Wildcard,
        })
    }
}
