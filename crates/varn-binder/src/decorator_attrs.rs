use super::Binder;
use varn_core::ast::decorator_kind::ContractDecorator;
use varn_core::ast::decorators::{decorator_head, match_builtin, BuiltinDecorator};
use varn_core::ast::types::Decorator;
use varn_core::ast::{Arg, ExprKind};
use varn_core::{Atom, AtomInterner};
use varn_sem::symbol::SymbolId;

fn head_is_throws(binder: &Binder, head: Atom) -> bool {
    ContractDecorator::of(binder.interner.resolve(head)) == Some(ContractDecorator::Throws)
}

fn throws_classes(
    arena: &varn_core::ast::AstArena,
    interner: &AtomInterner,
    d: &Decorator,
) -> Vec<String> {
    let ExprKind::Call { args, .. } = &arena.expr(d.expression).kind else {
        return Vec::new();
    };
    args.iter()
        .filter_map(|arg| {
            if let Arg::Positional(e) = arg
                && let ExprKind::Identifier { name } = &arena.expr(*e).kind
            {
                Some(interner.resolve(*name).to_string())
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn record_decorators(binder: &mut Binder, sym: SymbolId, decorators: &[Decorator]) {
    if decorators.is_empty() {
        return;
    }
    let scope = binder.current;
    binder
        .pending_decorator_roles
        .push((sym, decorators.to_vec(), scope));
}

impl Binder<'_> {
    pub(super) fn resolve_decorator_roles(&mut self) {
        let pending = std::mem::take(&mut self.pending_decorator_roles);
        for (symid, decorators, scope) in pending {
            for d in &decorators {
                let Some(head) = decorator_head(self.ast_arena, &self.interner, d) else {
                    continue;
                };
                if self.scopes.get(scope).resolve(head, &self.scopes).is_some() {
                    self.user_decorators.insert(d.range.start.offset);
                    continue;
                }
                if head_is_throws(self, head) {
                    let classes = throws_classes(self.ast_arena, &self.interner, d);
                    let sym = self.arena.get_mut(symid);
                    for c in classes {
                        if !sym.throws.contains(&c) {
                            sym.throws.push(c);
                        }
                    }
                    continue;
                }
                let mut matched =
                    match_builtin(self.ast_arena, &self.interner, std::slice::from_ref(d));
                let Some(m) = matched.pop() else {
                    continue;
                };
                let Ok(kind) = m.result.unwrap_or(Err("")) else {
                    continue;
                };
                let sym = self.arena.get_mut(symid);
                match kind {
                    BuiltinDecorator::Deprecated { message } => {
                        sym.deprecated = Some(message.unwrap_or_default());
                    }
                    BuiltinDecorator::Pure => sym.is_pure = true,
                    BuiltinDecorator::Inline => sym.force_inline = true,
                    BuiltinDecorator::Capability { domains } => {
                        for d in domains {
                            if !sym.capabilities.contains(&d) {
                                sym.capabilities.push(d);
                            }
                        }
                    }
                    BuiltinDecorator::Test => sym.is_test = true,
                }
            }
        }
    }
}
