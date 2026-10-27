use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::{Arg, ExprKind, StmtId, StmtKind};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};
use varn_sem::bind::{BindResult, TypeContext};
use varn_sem::types::ClassMemberKind;

impl<'r> Checker<'r> {
    pub(in super::super) fn check_super_call(
        &mut self,
        rec: &mut Recorder,
        args: &[Arg],
        range: &SourceRange,
        call_id: varn_core::ast::AstId,
        bind: &BindResult,
    ) {
        use crate::checker_expressions::members::member_util::class_type_params;
        let Some(parent) = self
            .current_class
            .as_ref()
            .and_then(|class| bind.class_parents.get(class.as_ref()))
        else {
            return;
        };
        if !class_type_params(self.resolver, &parent.name, parent.origin.as_ref(), bind).is_empty()
        {
            return;
        }
        let constructor = varn_sem::bind::BindView::new(bind, self.resolver)
            .get_class_members(&parent.name, parent.origin.as_deref())
            .and_then(|members| {
                members
                    .iter()
                    .find(|m| m.kind == ClassMemberKind::Constructor)
                    .map(|m| m.ty)
            });
        let Some(TypeKind::Fn(fid)) = constructor.map(|ty| self.ty_table.get(ty.0)) else {
            let owner = match parent.origin.as_deref() {
                Some(origin) => {
                    let Some(found) = self
                        .resolver
                        .module_bind(origin)
                        .or_else(|| self.resolver.stdlib_bind(origin))
                    else {
                        return;
                    };
                    crate::checker::foreign_classes::OwnerRef::Foreign(found)
                }
                None => crate::checker::foreign_classes::OwnerRef::Current(bind),
            };
            let ty = self.effective_ctor_ty(&parent.name, owner);
            let params = match self.ty_table.get(ty.0) {
                TypeKind::Fn(fid) => self.ty_table.get_function(fid).params.clone(),
                TypeKind::Primitive(_)
                | TypeKind::Literal(_)
                | TypeKind::This
                | TypeKind::Array(_)
                | TypeKind::Union(_)
                | TypeKind::Intersection(_)
                | TypeKind::Tuple(_)
                | TypeKind::Named(..)
                | TypeKind::Generic(..)
                | TypeKind::TemplateLiteral(_)
                | TypeKind::Object(_)
                | TypeKind::Record(_)
                | TypeKind::Typeof(_)
                | TypeKind::KeyOf(_)
                | TypeKind::IndexedAccess { .. }
                | TypeKind::Mapped { .. }
                | TypeKind::Conditional { .. }
                | TypeKind::Infer(_)
                | TypeKind::EnumVariant { .. }
                | TypeKind::TypePredicate { .. } => Vec::new(),
            };
            self.validate_call_arguments(rec, args, &params, range, call_id, bind);
            return;
        };
        let params = self.ty_table.get_function(fid).params.clone();
        self.validate_call_arguments(rec, args, &params, range, call_id, bind);
    }

    pub(crate) fn require_super_call(&mut self, body: StmtId, bind: &BindResult) {
        let Some(class) = self.current_class.clone() else {
            return;
        };
        if !bind.class_parents.contains_key(class.as_ref()) {
            return;
        }
        let arena = self.ast_arena;
        let calls_super = |stmt: &StmtId| {
            let StmtKind::Expr { expression } = &arena.stmt(*stmt).kind else {
                return false;
            };
            let ExprKind::Call { callee, .. } = &arena.expr(*expression).kind else {
                return false;
            };
            matches!(arena.expr(*callee).kind, ExprKind::Super)
        };
        let StmtKind::Block { stmts } = &arena.stmt(body).kind else {
            return;
        };
        if stmts.iter().any(calls_super) {
            return;
        }
        self.emit(
            Diagnostic::error(
                ErrorCode::ConstructorNotCalled,
                format!("the constructor of '{class}' must call 'super(...)': its parent is never built otherwise"),
            )
            .with_range(arena.stmt(body).range),
        );
    }
}
