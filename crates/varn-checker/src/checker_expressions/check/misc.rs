use super::const_int::closest_name;
use super::Checker;
use varn_core::ast::ExprId;
use varn_core::{Diagnostic, ErrorCode, Suggestion};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn check_pipeline(&mut self, left: ExprId, right: ExprId, bind: &BindResult) {
        self.check_expr(left, bind);
        let lhs_ty = self.infer_type(left, bind);
        let saved_pipeline = self.in_pipeline_rhs;
        let saved_pipe_ty = self.pipeline_value_type.replace(lhs_ty);
        self.in_pipeline_rhs = true;
        self.check_expr(right, bind);
        self.in_pipeline_rhs = saved_pipeline;
        self.pipeline_value_type = saved_pipe_ty;
    }

    pub(super) fn check_range(
        &mut self,
        start: ExprId,
        end: ExprId,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(start, bind);
        self.check_expr(end, bind);
        let lo = self.infer_type(start, bind).apparent(&self.ty_table);
        let hi = self.infer_type(end, bind).apparent(&self.ty_table);
        let domain_ok = |t: &Type| t.is_dynamic() || *t == Type::Int || *t == Type::Char;
        let same = lo.is_dynamic() || hi.is_dynamic() || lo == hi;
        if !(domain_ok(&lo) && domain_ok(&hi) && same) {
            let lo_s = lo.display(&self.ty_table, &bind.interner);
            let hi_s = hi.display(&self.ty_table, &bind.interner);
            self.emit(
                Diagnostic::error(
                    ErrorCode::TypeMismatch,
                    format!(
                        "range bounds must be both `int` or both `char`, found '{lo_s}' and '{hi_s}'"
                    ),
                )
                .with_range(range),
            );
        }
    }

    pub(super) fn check_tagged_template(
        &mut self,
        tag: ExprId,
        template: ExprId,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(tag, bind);
        self.check_expr(template, bind);
        let tag_ty_raw = self.infer_type(tag, bind);
        let tag_ty = tag_ty_raw.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        if let varn_core::TypeKind::Fn(fid) = self.ty_table.get(tag_ty.0) {
            let ret = self.ty_table.get_function(fid).return_type;
            self.record_type(range.start.offset, Type::resolved(ret));
        }
    }

    pub(super) fn check_with_object(
        &mut self,
        object: ExprId,
        properties: &[varn_core::ast::ObjectProp],
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(object, bind);
        for prop in properties {
            match prop {
                varn_core::ast::ObjectProp::Property { value, .. } => {
                    self.check_expr(*value, bind);
                }
                varn_core::ast::ObjectProp::Spread { argument, .. } => {
                    self.check_expr(*argument, bind);
                }
                varn_core::ast::ObjectProp::Method { .. }
                | varn_core::ast::ObjectProp::Getter { .. }
                | varn_core::ast::ObjectProp::Setter { .. } => {}
            }
        }
        let obj_ty = self.infer_type(object, bind);
        self.record_type(range.start.offset, obj_ty);
    }

    pub(super) fn check_meta_access(
        &mut self,
        target: ExprId,
        expr: ExprId,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(target, bind);
        let ty = self.infer_type(expr, bind);
        self.record_type(range.start.offset, ty);
    }

    pub(super) fn check_identifier(
        &mut self,
        name: varn_core::Atom,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        let name_str = bind.interner.resolve(name);
        if name_str == "_" {
            if !self.is_assignment_target && !self.in_pipeline_rhs {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::UnknownSymbol,
                        "cannot use '_' as a value; '_' is the discard placeholder",
                    )
                    .with_range(range),
                );
            } else if self.in_pipeline_rhs {
                if let Some(ty) = self.pipeline_value_type {
                    self.record_type(range.start.offset, ty);
                }
            }
            return;
        }

        let scope = bind.scopes.get(self.current_scope);
        if let Some(sid) = scope.resolve(name, &bind.scopes) {
            self.warn_if_deprecated(sid, name_str, range, bind);
        }
        if scope.resolve(name, &bind.scopes).is_none() && !self.is_assignment_target {
            let mut diag = Diagnostic::error(
                ErrorCode::UnknownSymbol,
                format!("undefined variable: {name_str}"),
            )
            .with_range(range);
            if let Some(candidate) = closest_name(name_str, scope, &bind.scopes, &bind.interner) {
                diag = diag.with_suggestion(Suggestion::did_you_mean(&candidate, range));
            }
            self.emit(diag);
        }
    }
}
