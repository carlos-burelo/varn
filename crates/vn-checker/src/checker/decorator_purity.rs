use super::Checker;
use vn_core::ast::decorators::{match_builtin, BuiltinDecorator};
use vn_core::ast::Decorator;
use vn_core::diagnostics::{Diagnostic, ErrorCode};
use vn_sem::bind::BindResult;

pub(super) fn is_pure_fn(
    decorators: &[Decorator],
    arena: &vn_core::ast::AstArena,
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
    arena: &vn_core::ast::AstArena,
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
    pub(crate) fn forbid_pure(&mut self, what: &str, range: vn_core::SourceRange) {
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
        name: vn_core::Atom,
        range: vn_core::SourceRange,
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
                    vn_sem::symbol::SymbolKind::Parameter
                        | vn_sem::symbol::SymbolKind::Let
                        | vn_sem::symbol::SymbolKind::Var
                        | vn_sem::symbol::SymbolKind::Const
                ) {
                    return;
                }
                break;
            }
            let sc = bind.scopes.get(scope);
            if sc.kind == vn_sem::scope::ScopeKind::Function {
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
        range: vn_core::SourceRange,
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
            vn_sem::symbol::SymbolKind::Function | vn_sem::symbol::SymbolKind::Method
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
        range: vn_core::SourceRange,
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
