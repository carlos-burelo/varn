use super::Checker;
use crate::binder::BindResult;
use crate::scope::ScopeId;
use crate::types::Type;
use varn_core::Diagnostic;

impl<'r> Checker<'r> {
    #[inline]
    pub(crate) fn emit(&mut self, diag: Diagnostic) {
        let diag = if diag.file.is_empty() {
            diag.with_file(self.source_file.clone())
        } else {
            diag
        };
        self.diagnostics.push(diag);
    }

    pub(crate) fn record_scope(&mut self, offset: u32) {
        if self.record_expr_types {
            self.node_scopes.insert(offset, self.current_scope);
        }
    }

    pub(crate) fn record_scope_span(&mut self, start: u32, end: u32, scope: ScopeId) {
        if self.record_expr_types {
            self.scope_spans
                .push(super::ScopeSpan { start, end, scope });
            self.node_scopes.insert(start, scope);
        }
    }

    pub(crate) fn with_next_child_scope<R>(
        &mut self,
        bind: &BindResult,
        offset: u32,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved_scope = self.current_scope;
        if let Some(child) = self.next_child_scope(bind) {
            self.current_scope = child;
            self.record_scope(offset);
        }
        let res = f(self);
        self.current_scope = saved_scope;
        res
    }

    pub(crate) fn with_next_child_scope_span<R>(
        &mut self,
        bind: &BindResult,
        start: u32,
        end: u32,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved_scope = self.current_scope;
        if let Some(child) = self.next_child_scope(bind) {
            self.current_scope = child;
            self.record_scope_span(start, end, child);
        }
        let res = f(self);
        self.current_scope = saved_scope;
        res
    }

    pub(crate) fn with_expected<R>(
        &mut self,
        ty: Option<Type>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prev = self.expected_type.take();
        self.expected_type = ty;
        let result = f(self);
        self.expected_type = prev;
        result
    }

    pub(crate) fn in_function_body<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved_in_function = self.in_function;
        let saved_loop_depth = self.loop_depth;
        let saved_switch_depth = self.switch_depth;
        self.in_function = true;
        self.loop_depth = 0;
        self.switch_depth = 0;

        let result = f(self);

        self.in_function = saved_in_function;
        self.loop_depth = saved_loop_depth;
        self.switch_depth = saved_switch_depth;
        result
    }

    pub(crate) fn next_child_scope(&mut self, bind: &BindResult) -> Option<ScopeId> {
        let children = &bind.scopes.get(self.current_scope).children;
        let idx = self.child_indices.entry(self.current_scope).or_insert(0);
        if *idx < children.len() {
            let child_id = children[*idx];
            *idx += 1;
            Some(child_id)
        } else {
            None
        }
    }
}
