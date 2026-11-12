use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use vn_core::ast::{ExprId, ExprKind};
use vn_core::{Diagnostic, ErrorCode, TypeKind};
use vn_sem::bind::{BindResult, BindView};
use vn_sem::symbol::SymbolKind;
use vn_sem::types::{ObjectTypeMember, Type, TypeContext};

impl<'r> Checker<'r> {
    pub(super) fn is_value_product(&mut self, ty: &Type, bind: &BindResult) -> bool {
        let ty = ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        match self.ty_table.get(ty.0) {
            TypeKind::Record(_) => true,
            TypeKind::Named(name, origin) | TypeKind::Generic(name, _, origin) => {
                let name = self.resolve_bind_atom(bind, name);
                let origin = origin.map(|o| self.resolve_bind_atom(bind, o));
                BindView::new(bind, self.resolver)
                    .class_entry(&name, origin.as_deref())
                    .is_some_and(|entry| entry.is_struct)
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Array(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => false,
        }
    }

    pub(super) fn plan_value_update(
        &mut self,
        rec: &mut Recorder,
        target: ExprId,
        bind: &BindResult,
    ) -> bool {
        let arena = self.ast_arena;
        let ExprKind::Member {
            object,
            computed: false,
            optional: false,
            ..
        } = &arena.expr(target).kind
        else {
            return false;
        };
        let object_ty = self.infer_type(rec, *object, bind);
        if !self.is_value_product(&object_ty, bind) {
            return false;
        }
        self.plan_value_field(rec, target, bind);
        true
    }

    fn plan_value_field(&mut self, rec: &mut Recorder, member: ExprId, bind: &BindResult) {
        let arena = self.ast_arena;
        let ExprKind::Member {
            object, property, ..
        } = &arena.expr(member).kind
        else {
            return;
        };
        let (object, property) = (*object, *property);
        rec.desugar.value_field_places.insert(member.index());
        let range = arena.expr(member).range;
        let object_ty = self.infer_type(rec, object, bind);
        let non_null = object_ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        let is_record = matches!(self.ty_table.get(non_null.0), TypeKind::Record(_));
        if is_record && let ExprKind::Identifier { name } = &arena.expr(property).kind {
            let field = bind.interner.resolve(*name);
            if matches!(
                self.find_member(&object_ty, field, bind),
                Some(ObjectTypeMember::Property { readonly: true, .. })
            ) {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::NotAssignable,
                        format!("cannot assign to readonly property '{field}'"),
                    )
                    .with_range(range),
                );
            }
        }
        self.plan_value_root(rec, object, bind);
    }

    fn plan_value_root(&mut self, rec: &mut Recorder, place: ExprId, bind: &BindResult) {
        let arena = self.ast_arena;
        let range = arena.expr(place).range;
        match &arena.expr(place).kind {
            ExprKind::Identifier { name } => {
                let scope = bind.scopes.get(self.current_scope);
                let kind = scope
                    .resolve(*name, &bind.scopes)
                    .map(|id| bind.arena.get(id).kind);
                let refusal = match kind {
                    Some(SymbolKind::Let | SymbolKind::Var | SymbolKind::Parameter) => None,
                    Some(SymbolKind::Const) => Some(format!(
                        "cannot update a field of constant '{}': a record is a value, declare it with `let` or build a new one with `with`",
                        bind.interner.resolve(*name)
                    )),
                    Some(
                        SymbolKind::Class
                        | SymbolKind::Enum
                        | SymbolKind::Function
                        | SymbolKind::Interface
                        | SymbolKind::TypeAlias
                        | SymbolKind::Property
                        | SymbolKind::Method
                        | SymbolKind::TypeParameter
                        | SymbolKind::Namespace
                        | SymbolKind::Extension
                        | SymbolKind::EnumMember,
                    ) => Some(format!(
                        "cannot update a field of '{}': it is not a variable",
                        bind.interner.resolve(*name)
                    )),
                    None => None,
                };
                if let Some(message) = refusal {
                    self.emit(
                        Diagnostic::error(ErrorCode::NotAssignable, message).with_range(range),
                    );
                }
            }
            ExprKind::Member {
                object,
                property,
                computed: false,
                optional: false,
                ..
            } => {
                let (object, property) = (*object, *property);
                let object_ty = self.infer_type(rec, object, bind);
                if self.is_value_product(&object_ty, bind) {
                    self.plan_value_field(rec, place, bind);
                    return;
                }
                if let ExprKind::Identifier { name } = &arena.expr(property).kind {
                    let field = bind.interner.resolve(*name);
                    let frozen = matches!(
                        self.find_member(&object_ty, field, bind),
                        Some(ObjectTypeMember::Property { readonly: true, .. })
                    ) || self.declared_readonly(&object_ty, field, bind);
                    if frozen {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::NotAssignable,
                                format!("cannot assign to readonly property '{field}'"),
                            )
                            .with_range(range),
                        );
                    }
                }
                self.require_repeatable_place(object, range);
            }
            ExprKind::Member {
                object,
                property,
                computed: true,
                optional: false,
                ..
            } => {
                let (object, property) = (*object, *property);
                let object_ty = self.infer_type(rec, object, bind);
                let container_is_value = matches!(
                    self.ty_table.get(object_ty.0),
                    TypeKind::Tuple(_) | TypeKind::Primitive(vn_core::LangPrimitive::Str)
                ) || self.is_value_product(&object_ty, bind);
                if container_is_value {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::NotAssignable,
                            "cannot update through an element of a tuple, a record or a string: they are values",
                        )
                        .with_range(range),
                    );
                    return;
                }
                self.require_repeatable_place(object, range);
                self.require_repeatable_place(property, range);
            }
            ExprKind::This => {
                let this_ty = self.infer_type(rec, place, bind);
                if self.is_value_product(&this_ty, bind) {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::NotAssignable,
                            "a struct method cannot update the fields of `this`: return a new value with `with`",
                        )
                        .with_range(range),
                    );
                }
            }
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::ImportMeta(_)
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. }
            | ExprKind::Member { .. } => {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::NotAssignable,
                        "a field of a value can only be updated through a variable, a field or an element: bind the value to a `let` first",
                    )
                    .with_range(range),
                );
            }
        }
    }

    fn require_repeatable_place(&mut self, expr: ExprId, at: vn_core::SourceRange) {
        if !self.is_repeatable(expr) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotAssignable,
                    "a field of a value is updated by reading and writing its place, so the place cannot contain a call or any other effect: bind it to a `let` first",
                )
                .with_range(at),
            );
        }
    }

    fn is_repeatable(&self, expr: ExprId) -> bool {
        let kind = &self.ast_arena.expr(expr).kind;
        if let ExprKind::Member {
            object,
            property,
            computed,
            optional: false,
            ..
        } = kind
        {
            return self.is_repeatable(*object) && (!*computed || self.is_repeatable(*property));
        }
        matches!(
            kind,
            ExprKind::Identifier { .. }
                | ExprKind::This
                | ExprKind::IntLiteral { .. }
                | ExprKind::StrLiteral { .. }
                | ExprKind::CharLiteral { .. }
                | ExprKind::BoolLiteral { .. }
        )
    }
}
