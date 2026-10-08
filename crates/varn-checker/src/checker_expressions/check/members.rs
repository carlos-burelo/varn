use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use crate::checker_expressions::name_suggestions::closest_in_list;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, Suggestion, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn check_member_expr(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        object: ExprId,
        property: ExprId,
        computed: bool,
        optional: bool,
        range: &SourceRange,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        let property_range = arena.expr(property).range;
        self.check_expr(rec, object, bind);
        if computed {
            self.check_member_index(rec, object, property, property_range, bind);
            return;
        }

        let ExprKind::Identifier { name: prop_name } = &arena.expr(property).kind else {
            let prop_ty = self.infer_type(rec, expr, bind);
            self.record_type(rec, property_range.start.offset, prop_ty);
            return;
        };
        let prop_name = bind.interner.resolve(*prop_name);

        let obj_ty = self.infer_type(rec, object, bind);
        if !optional && obj_ty.is_nullable(&self.ty_table) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::PossibleNullDereference,
                    format!(
                        "object is possibly null: cannot access property '{}' on nullable type '{}'",
                        prop_name,
                        obj_ty.display(&self.ty_table, &bind.interner)
                    ),
                )
                .with_suggestion(Suggestion::new(
                    "use optional chaining '?.' to safely access properties on a nullable object",
                ))
                .with_range(*range),
            );
        }

        let check_ty = obj_ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));

        if let Some((ty, maybe_sid)) = self.find_member_info(&check_ty, prop_name, bind) {
            if let Some(sid) = maybe_sid {
                if sid < bind.arena.len()
                    && bind.interner.get(prop_name) == Some(bind.arena.get(sid).name)
                {
                    self.warn_if_deprecated(sid, prop_name, property_range, bind);
                }
                self.record_member_type(rec, property_range.start.offset, ty, sid);
            } else {
                self.record_type(rec, property_range.start.offset, ty);
            }
        } else {
            let prop_ty = self.infer_type(rec, expr, bind);
            self.record_type(rec, property_range.start.offset, prop_ty);
        }
        let should_check = !matches!(
            self.ty_table.get(check_ty.0),
            TypeKind::Primitive(varn_core::LangPrimitive::Never)
        );

        if let Some(tn) = extension_type_name(self, &check_ty, &self.ty_table, bind) {
            if let Some(getter_map) = bind.extensions.getters.get(tn.as_ref()) {
                if let Some(mangled) = getter_map.get(prop_name) {
                    rec.desugar
                        .extension_members
                        .insert(property_range.start.offset, mangled.clone());
                }
            } else if let Some(method_map) = bind.extensions.methods.get(tn.as_ref()) {
                if let Some(mangled) = method_map.get(prop_name) {
                    rec.desugar
                        .extension_members
                        .insert(property_range.start.offset, mangled.clone());
                }
            }
        }

        if should_check && !self.member_exists_cached(&check_ty, prop_name, bind) {
            let candidates = self.collect_member_names(&check_ty, bind);
            let suggestion = closest_in_list(prop_name, &candidates)
                .map(|c| Suggestion::did_you_mean(c, *range));
            let mut diag = Diagnostic::error(
                ErrorCode::MissingProperty,
                format!(
                    "property '{prop_name}' does not exist on type '{}'",
                    check_ty.display(&self.ty_table, &bind.interner)
                ),
            )
            .with_range(*range);
            if let Some(s) = suggestion {
                diag = diag.with_suggestion(s);
            }
            self.emit(diag);
        }

        if rec.enabled {
            let final_mem_ty = self
                .find_member_info(&check_ty, prop_name, bind)
                .map(|(t, _)| t)
                .unwrap_or_else(|| self.infer_type(rec, expr, bind));

            let is_static = if let ExprKind::Identifier { name } = &arena.expr(object).kind {
                bind.scopes
                    .get(bind.global_scope)
                    .resolve(*name, &bind.scopes)
                    .map(|sid| {
                        matches!(
                            bind.arena.get(sid).kind,
                            varn_sem::symbol::SymbolKind::Class
                                | varn_sem::symbol::SymbolKind::Interface
                                | varn_sem::symbol::SymbolKind::Enum
                                | varn_sem::symbol::SymbolKind::Namespace
                                | varn_sem::symbol::SymbolKind::Struct
                        )
                    })
                    .unwrap_or(false)
            } else {
                false
            };

            let check_kind = self.ty_table.get(check_ty.0);
            let is_enum = matches!(check_kind, TypeKind::EnumVariant { .. })
                || if let TypeKind::Named(n, _) = check_kind {
                    let n_str = self.resolve_bind_atom(bind, n);
                    bind.interner
                        .get(&n_str)
                        .and_then(|atom| {
                            bind.scopes
                                .get(bind.global_scope)
                                .resolve(atom, &bind.scopes)
                        })
                        .map(|sid| bind.arena.get(sid).kind == varn_sem::symbol::SymbolKind::Enum)
                        .unwrap_or(false)
                } else {
                    false
                };

            let final_mem_kind = self.ty_table.get(final_mem_ty.0);
            let member_kind = if is_enum {
                varn_sem::semantic_info::ResolvedMemberKind::EnumMember
            } else if rec
                .desugar
                .extension_members
                .contains_key(&property_range.start.offset)
            {
                if matches!(final_mem_kind, TypeKind::Fn(_)) {
                    varn_sem::semantic_info::ResolvedMemberKind::ExtensionMethod
                } else {
                    varn_sem::semantic_info::ResolvedMemberKind::ExtensionProperty
                }
            } else if is_static {
                if matches!(final_mem_kind, TypeKind::Fn(_)) {
                    varn_sem::semantic_info::ResolvedMemberKind::StaticMethod
                } else {
                    varn_sem::semantic_info::ResolvedMemberKind::StaticProperty
                }
            } else if matches!(final_mem_kind, TypeKind::Fn(_)) {
                varn_sem::semantic_info::ResolvedMemberKind::Method
            } else {
                varn_sem::semantic_info::ResolvedMemberKind::Property
            };

            let origin_module = match check_kind {
                TypeKind::Named(_, orig) | TypeKind::Generic(_, _, orig) => {
                    orig.map(|o| self.resolve_bind_atom(bind, o))
                }
                TypeKind::Primitive(p) => p.core_module().map(std::sync::Arc::from),
                TypeKind::Literal(l) => l.base().core_module().map(std::sync::Arc::from),
                TypeKind::Builtin(b) => Some(std::sync::Arc::from(b.core_module())),
                TypeKind::This
                | TypeKind::Array(_)
                | TypeKind::Union(_)
                | TypeKind::Intersection(_)
                | TypeKind::Tuple(_)
                | TypeKind::TemplateLiteral(_)
                | TypeKind::Fn(_)
                | TypeKind::Object(_)
                | TypeKind::Typeof(_)
                | TypeKind::KeyOf(_)
                | TypeKind::IndexedAccess { .. }
                | TypeKind::Mapped { .. }
                | TypeKind::Conditional { .. }
                | TypeKind::Infer(_)
                | TypeKind::EnumVariant { .. }
                | TypeKind::TypePredicate { .. } => None,
            };

            rec.member_resolutions.insert(
                property_range.start.offset,
                varn_sem::semantic_info::MemberResolution {
                    receiver_ty: check_ty,
                    member_name: std::sync::Arc::from(prop_name),
                    member_kind,
                    member_ty: final_mem_ty,
                    origin_module,
                    def_range: None,
                    doc: None,
                },
            );
        }

        let obj_kind = self.ty_table.get(obj_ty.0);
        let class_name = match obj_kind {
            TypeKind::Named(n, _origin) | TypeKind::Generic(n, _, _origin) => {
                Some(self.resolve_bind_atom(bind, n).to_string())
            }
            TypeKind::Primitive(_)
            | TypeKind::Builtin(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Object(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => None,
        };
        if let Some(class_name) = class_name {
            self.check_member_visibility(&class_name, prop_name, range, bind);
        }
    }
}

pub(crate) fn extension_type_name(
    checker: &Checker,
    ty: &Type,
    table: &varn_sem::types::CheckerTyTable,
    bind: &BindResult,
) -> Option<std::sync::Arc<str>> {
    match table.get(ty.0) {
        TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => {
            Some(checker.resolve_bind_atom(bind, n))
        }
        k @ (TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_)) => {
            k.lang_name().map(std::sync::Arc::from)
        }
        TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Object(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => None,
    }
}
