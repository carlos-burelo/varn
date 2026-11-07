use super::recorder::Recorder;
use super::Checker;
use varn_core::Diagnostic;
use varn_sem::bind::BindResult;
use varn_sem::scope::ScopeId;
use varn_sem::types::Type;

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

    pub(crate) fn diagnostics_mark(&self) -> usize {
        self.diagnostics.len()
    }

    pub(crate) fn errors_since(&self, mark: usize) -> bool {
        self.diagnostics.all()[mark..].iter().any(|d| d.is_error())
    }

    pub(crate) fn record_scope(&mut self, rec: &mut Recorder, offset: u32) {
        rec.record_scope(offset, self.current_scope);
    }

    pub(crate) fn record_scope_span(
        &mut self,
        rec: &mut Recorder,
        start: u32,
        end: u32,
        scope: ScopeId,
    ) {
        rec.record_scope_span(start, end, scope);
    }

    pub(crate) fn with_next_child_scope<R>(
        &mut self,
        rec: &mut Recorder,
        bind: &BindResult,
        offset: u32,
        f: impl FnOnce(&mut Self, &mut Recorder) -> R,
    ) -> R {
        let saved_scope = self.current_scope;
        if let Some(child) = self.next_child_scope(bind) {
            self.current_scope = child;
            self.record_scope(rec, offset);
        }
        let res = f(self, rec);
        self.current_scope = saved_scope;
        res
    }

    pub(crate) fn with_next_child_scope_span<R>(
        &mut self,
        rec: &mut Recorder,
        bind: &BindResult,
        start: u32,
        end: u32,
        f: impl FnOnce(&mut Self, &mut Recorder) -> R,
    ) -> R {
        let saved_scope = self.current_scope;
        if let Some(child) = self.next_child_scope(bind) {
            self.current_scope = child;
            self.record_scope_span(rec, start, end, child);
        }
        let res = f(self, rec);
        self.current_scope = saved_scope;
        res
    }

    pub(crate) fn with_expected<R>(
        &mut self,
        ty: Option<Type>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prev = self.expected_type.take();
        let prev_pending = self.expected_pending;
        let prev_owner = self.expected_owner;
        let prev_forwarded = std::mem::take(&mut self.expected_forwarded);
        self.expected_type = ty;
        self.expected_pending = ty.is_some();
        let result = f(self);
        self.expected_type = prev;
        self.expected_pending = prev_pending;
        self.expected_owner = prev_owner;
        self.expected_forwarded = prev_forwarded;
        result
    }

    pub(crate) fn in_function_body<R>(
        &mut self,
        is_async: bool,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved_in_function = self.in_function;
        let saved_in_async = self.in_async;
        self.in_async = is_async;
        let saved_loop_depth = self.loop_depth;
        let saved_switch_depth = self.switch_depth;
        let saved_labels = std::mem::take(&mut self.loop_labels);
        let saved_narrowed = std::mem::take(&mut self.narrowed_types);
        let saved_members = std::mem::take(&mut self.member_narrowed);
        self.in_function = true;
        self.loop_depth = 0;
        self.switch_depth = 0;

        let result = f(self);

        self.in_function = saved_in_function;
        self.in_async = saved_in_async;
        self.loop_depth = saved_loop_depth;
        self.switch_depth = saved_switch_depth;
        self.loop_labels = saved_labels;
        self.narrowed_types = saved_narrowed;
        self.member_narrowed = saved_members;
        self.mark_infer_env_dirty();
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

impl Checker<'_> {
    pub(crate) fn infer_expecting(
        &mut self,
        rec: &mut Recorder,
        expr: varn_core::ast::ExprId,
        expected: Type,
        bind: &varn_sem::bind::BindResult,
    ) -> Type {
        let prev = self.expected_type.take();
        let prev_pending = self.expected_pending;
        let prev_owner = self.expected_owner;
        let prev_forwarded = std::mem::take(&mut self.expected_forwarded);
        self.expected_type = Some(expected);
        self.expected_pending = false;
        self.expected_owner = Some(expr.index());
        self.expected_forwarded = self.forwarded_operands(expr);
        let result = self.infer_type_internal(rec, expr, bind);
        self.infer_cache
            .insert((expr, self.current_scope, self.infer_env_rev), result);
        self.expected_type = prev;
        self.expected_pending = prev_pending;
        self.expected_owner = prev_owner;
        self.expected_forwarded = prev_forwarded;
        result
    }
}
