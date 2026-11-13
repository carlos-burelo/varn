use super::Binder;
use std::sync::Arc;
use vn_core::ast::{Arg, ExprKind, FunctionDecl, Pattern, StmtKind};
use vn_sem::symbol::Forward;

impl Binder<'_> {
    pub(super) fn forwarder_of(&self, f: &FunctionDecl) -> Option<Arc<Forward>> {
        if f.modifiers.is_async
            || f.modifiers.is_generator
            || f.modifiers.is_declare
            || !f.decorators.is_empty()
            || f.params.is_empty()
        {
            return None;
        }
        let mut names = Vec::with_capacity(f.params.len());
        for param in &f.params {
            let Pattern::Identifier { name, .. } = &param.pattern else {
                return None;
            };
            if param.is_rest || param.default.is_some() {
                return None;
            }
            names.push(*name);
        }
        let StmtKind::Block { stmts } = &self.ast_arena.stmt(f.body).kind else {
            return None;
        };
        let [only] = stmts.as_slice() else {
            return None;
        };
        let StmtKind::Return {
            argument: Some(mut value),
        } = self.ast_arena.stmt(*only).kind
        else {
            return None;
        };
        while let ExprKind::As { expression, .. } = &self.ast_arena.expr(value).kind {
            value = *expression;
        }
        let ExprKind::Call {
            callee,
            type_args,
            args,
            optional: false,
        } = &self.ast_arena.expr(value).kind
        else {
            return None;
        };
        if !type_args.is_empty() || args.is_empty() || args.len() > names.len() {
            return None;
        }
        for (slot, arg) in args.iter().enumerate() {
            let Arg::Positional(expr) = arg else {
                return None;
            };
            let ExprKind::Identifier { name } = &self.ast_arena.expr(*expr).kind else {
                return None;
            };
            if *name != names[slot] {
                return None;
            }
        }
        let ExprKind::Identifier { name: callee } = &self.ast_arena.expr(*callee).kind else {
            return None;
        };
        if names.contains(callee) {
            return None;
        }
        let imported = self.arena.get(self.scopes.get(0).lookup(*callee)?);
        let module = self.interner.resolve(imported.origin_module?);
        let symbol = self.interner.resolve(imported.original_name?);
        Some(Arc::new(Forward {
            module: Arc::from(module),
            symbol: Arc::from(symbol),
            args: u8::try_from(args.len()).ok()?,
        }))
    }
}
