use super::members::extension_type_name;
use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::operators::Visibility;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::types::ObjectTypeMember;

impl<'r> Checker<'r> {
    pub(super) fn check_extension_assignment(
        &mut self,
        rec: &mut Recorder,
        target: ExprId,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        let target_range = arena.expr(target).range;
        if let ExprKind::Member {
            object,
            computed: true,
            ..
        } = &arena.expr(target).kind
        {
            let obj_ty = self.infer_type(rec, *object, bind);
            if matches!(self.ty_table.get(obj_ty.0), TypeKind::Tuple(_)) {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::NotAssignable,
                        "cannot assign to a tuple element: tuples are immutable",
                    )
                    .with_range(target_range),
                );
            }
            return;
        }
        let ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } = &arena.expr(target).kind
        else {
            return;
        };
        let (object, property) = (*object, *property);
        let ExprKind::Identifier { name: prop_name } = &arena.expr(property).kind else {
            return;
        };
        let prop_name = bind.interner.resolve(*prop_name);

        let obj_ty = self.infer_type(rec, object, bind);
        let non_null = obj_ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        if let Some(tn) = extension_type_name(self, &non_null, &self.ty_table, bind) {
            if let Some(setter_map) = bind.extensions.setters.get(tn.as_ref()) {
                if let Some(mangled) = setter_map.get(prop_name) {
                    rec.desugar
                        .extension_set_members
                        .insert(target_range.start.offset, mangled.clone());
                }
            }
        }
        if let Some(ObjectTypeMember::Property { readonly: true, .. }) =
            self.find_member(&obj_ty, prop_name, bind)
        {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotAssignable,
                    format!("cannot assign to readonly property '{prop_name}'"),
                )
                .with_range(target_range),
            );
        }
    }

    pub(super) fn check_member_visibility(
        &mut self,
        class_name: &str,
        prop_name: &str,
        range: &SourceRange,
        bind: &BindResult,
    ) {
        let Some(members) = bind.get_class_entry(class_name) else {
            return;
        };
        let Some(m) = members
            .members
            .iter()
            .find(|m| m.name.as_ref() == prop_name)
        else {
            return;
        };

        match m.visibility {
            Some(Visibility::Private) => {
                if self.current_class.as_deref() != Some(class_name) {
                    self.emit(
                        Diagnostic::error(ErrorCode::PrivateMemberAccess, format!(
                            "property '{prop_name}' is private and only accessible within class '{class_name}'"
                        ))
                        .with_range(*range),
                    );
                }
            }
            Some(Visibility::Protected) => {
                let current_class = self.current_class.as_deref();
                let is_authorized = current_class.is_some_and(|c| {
                    c == class_name || self.is_subclass_or_same(c, class_name, bind)
                });
                if !is_authorized {
                    self.emit(
                        Diagnostic::error(ErrorCode::ProtectedMemberAccess, format!(
                            "property '{prop_name}' is protected and only accessible within class '{class_name}' and its subclasses"
                        ))
                        .with_range(*range),
                    );
                }
            }
            None | Some(Visibility::Public) => {}
        }
    }
}
