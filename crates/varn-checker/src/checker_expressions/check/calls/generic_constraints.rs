use crate::checker::Checker;
use varn_core::ast::{ExprId, ExprKind, TypeNode};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn check_type_arg_constraints(
        &mut self,
        callee: ExprId,
        type_args: &[TypeNode],
        range: &SourceRange,
        bind: &BindResult,
    ) {
        if type_args.is_empty() {
            return;
        }
        let resolved: Vec<Option<Type>> = type_args
            .iter()
            .map(|a| Some(self.resolve_type_node_cached(a, bind)))
            .collect();
        self.check_supplied_constraints(callee, &resolved, range, bind);
    }

    pub(super) fn check_inferred_constraints(
        &mut self,
        callee: ExprId,
        mapping: &rustc_hash::FxHashMap<std::sync::Arc<str>, Type>,
        range: &SourceRange,
        bind: &BindResult,
    ) {
        let ExprKind::Identifier { name: fn_name } = &self.ast_arena.expr(callee).kind else {
            return;
        };
        let Some(fn_sym) =
            resolve_function_symbol(bind.interner.resolve(*fn_name), self.current_scope, bind)
        else {
            return;
        };
        let inferred: Vec<Option<Type>> = fn_sym
            .type_params
            .iter()
            .map(|param| {
                mapping
                    .get(bind.interner.resolve(*param))
                    .copied()
                    .filter(|ty| !ty.is_dynamic())
            })
            .collect();
        self.check_supplied_constraints(callee, &inferred, range, bind);
    }

    fn check_supplied_constraints(
        &mut self,
        callee: ExprId,
        resolved: &[Option<Type>],
        range: &SourceRange,
        bind: &BindResult,
    ) {
        let ExprKind::Identifier { name: fn_name } = &self.ast_arena.expr(callee).kind else {
            return;
        };
        let Some(fn_sym) =
            resolve_function_symbol(bind.interner.resolve(*fn_name), self.current_scope, bind)
        else {
            return;
        };

        for (i, constraint) in fn_sym.type_param_constraints.iter().enumerate() {
            if let (Some(ct), Some(supplied)) =
                (constraint, resolved.get(i).copied().flatten().as_ref())
                && !self.types_compatible_cached(ct, supplied, Some(bind))
            {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::ConstraintViolation,
                        format!(
                            "type '{}' does not satisfy constraint '{}'",
                            supplied.display(&self.ty_table, &bind.interner),
                            ct.display(&self.ty_table, &bind.interner)
                        ),
                    )
                    .with_range(*range),
                );
            }
        }
    }
}

fn resolve_function_symbol<'a>(
    name: &str,
    current_scope: varn_sem::scope::ScopeId,
    bind: &'a BindResult,
) -> Option<&'a varn_sem::symbol::Symbol> {
    let current = bind.scopes.get(current_scope);
    if let Some(id) = bind
        .interner
        .get(name)
        .and_then(|atom| current.resolve(atom, &bind.scopes))
    {
        let sym = bind.arena.get(id);
        if matches!(sym.kind, varn_sem::symbol::SymbolKind::Function) {
            return Some(sym);
        }
    }

    let global = bind.scopes.get(bind.global_scope);
    if let Some(id) = bind
        .interner
        .get(name)
        .and_then(|atom| global.resolve(atom, &bind.scopes))
    {
        let sym = bind.arena.get(id);
        if matches!(sym.kind, varn_sem::symbol::SymbolKind::Function) {
            return Some(sym);
        }
    }

    None
}
