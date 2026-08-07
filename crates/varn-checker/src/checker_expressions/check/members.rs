use crate::binder::BindResult;
use crate::checker::Checker;
use crate::checker_expressions::helpers::closest_in_list;
use crate::types::{ObjectTypeMember, Type};
use varn_core::ast::operators::Visibility;
use varn_core::ast::{Expr, ExprKind};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, Suggestion, TypeKind};

impl Checker {
    pub(super) fn check_extension_assignment(&mut self, target: &Expr, bind: &BindResult) {
        let ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } = &target.kind
        else {
            return;
        };
        let ExprKind::Identifier { name: prop_name } = &property.kind else {
            return;
        };

        let obj_ty = self.infer_type(object, bind);
        if let Some(tn) = extension_type_name(&obj_ty.non_nullified()) {
            if let Some(setter_map) = bind.extensions.setters.get(tn.as_ref()) {
                if let Some(mangled) = setter_map.get(prop_name.as_ref()) {
                    self.extension_set_members
                        .insert(target.range.start.offset, mangled.clone());
                }
            }
        }
        if let Some(ObjectTypeMember::Property { readonly: true, .. }) =
            self.find_member(&obj_ty, prop_name.as_ref(), bind)
        {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotAssignable,
                    format!("cannot assign to readonly property '{prop_name}'"),
                )
                .with_range(target.range),
            );
        }
    }

    pub(super) fn check_member_expr(
        &mut self,
        expr: &Expr,
        object: &Expr,
        property: &Expr,
        computed: bool,
        optional: bool,
        range: &SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(object, bind);
        if computed {
            self.check_expr(property, bind);
            return;
        }

        let ExprKind::Identifier { name: prop_name } = &property.kind else {
            let prop_ty = self.infer_type(expr, bind);
            self.record_type(property.range.start.offset, prop_ty);
            return;
        };

        let obj_ty = self.infer_type(object, bind);
        if !optional && obj_ty.is_nullable() {
            self.emit(
                Diagnostic::error(
                    ErrorCode::PossibleNullDereference,
                    format!(
                        "object is possibly null: cannot access property '{}' on nullable type '{}'",
                        prop_name, obj_ty
                    ),
                )
                .with_suggestion(Suggestion::new(
                    "use optional chaining '?.' to safely access properties on a nullable object",
                ))
                .with_range(*range),
            );
        }

        let check_ty = obj_ty.non_nullified();

        if let Some((ty, maybe_sid)) = self.find_member_info(&check_ty, prop_name.as_ref(), bind) {
            if let Some(sid) = maybe_sid {
                self.record_member_type(property.range.start.offset, ty, sid);
            } else {
                self.record_type(property.range.start.offset, ty);
            }
        } else {
            let prop_ty = self.infer_type(expr, bind);
            self.record_type(property.range.start.offset, prop_ty);
        }
        let should_check = !matches!(check_ty.0, TypeKind::Intrinsic(varn_core::TypeTag::Never));

        if let Some(tn) = extension_type_name(&check_ty) {
            if let Some(getter_map) = bind.extensions.getters.get(tn.as_ref()) {
                if let Some(mangled) = getter_map.get(prop_name.as_ref()) {
                    self.extension_members
                        .insert(property.range.start.offset, mangled.clone());
                }
            } else if let Some(method_map) = bind.extensions.methods.get(tn.as_ref()) {
                if let Some(mangled) = method_map.get(prop_name.as_ref()) {
                    self.extension_members
                        .insert(property.range.start.offset, mangled.clone());
                }
            }
        }

        if should_check && !self.member_exists_cached(&check_ty, prop_name.as_ref(), bind) {
            let candidates = self.collect_member_names(&check_ty, bind);
            let suggestion = closest_in_list(prop_name.as_ref(), &candidates)
                .map(|c| Suggestion::did_you_mean(c, *range));
            let mut diag = Diagnostic::error(
                ErrorCode::MissingProperty,
                format!("property '{prop_name}' does not exist on type '{check_ty}'"),
            )
            .with_range(*range);
            if let Some(s) = suggestion {
                diag = diag.with_suggestion(s);
            }
            self.emit(diag);
        }

        let class_name = match &obj_ty.0 {
            TypeKind::Named(n, _origin) | TypeKind::Generic(n, _, _origin) => Some(n.as_ref()),
            _ => None,
        };
        if let Some(class_name) = class_name {
            self.check_member_visibility(class_name, prop_name.as_ref(), range, bind);
        }
    }

    fn check_member_visibility(
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
            _ => {}
        }
    }
}

pub(crate) fn extension_type_name(ty: &Type) -> Option<std::rc::Rc<str>> {
    match &ty.0 {
        TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => Some(n.clone()),
        TypeKind::Intrinsic(tag) => Some(std::rc::Rc::from(tag.name())),
        _ => None,
    }
}
