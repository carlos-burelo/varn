use super::recorder::Recorder;
use super::Checker;
use std::collections::BTreeSet;
use varn_core::ast::{BinaryOp, ExprId};
use varn_core::{Diagnostic, ErrorCode, LangPrimitive, SourceRange, TypeKind};
use varn_sem::bind::{BindResult, BindView};
use varn_sem::types::{CheckerTyId, Type, TypeContext};

impl Checker<'_> {
    pub(crate) fn reject_reference_equality(
        &mut self,
        rec: &mut Recorder,
        op: BinaryOp,
        left: ExprId,
        right: ExprId,
        range: SourceRange,
        bind: &BindResult,
    ) -> bool {
        if !matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
            return false;
        }
        let left = self.infer_type(rec, left, bind);
        let right = self.infer_type(rec, right, bind);
        if left == Type::Null || right == Type::Null || left.is_error() || right.is_error() {
            return false;
        }
        let view = BindView::new(bind, self.resolver).at(self.current_scope);
        if self.equality_value_type(left, &view, &mut BTreeSet::new())
            && self.equality_value_type(right, &view, &mut BTreeSet::new())
        {
            return false;
        }
        self.emit(Diagnostic::error(ErrorCode::InvalidTypeOperator,
            "equality requires value types; use sameValue(left, right) or an explicit equals method")
            .with_range(range));
        true
    }

    fn equality_value_type(
        &self,
        ty: Type,
        view: &BindView<'_>,
        path: &mut BTreeSet<CheckerTyId>,
    ) -> bool {
        if path.len() >= 64 || !path.insert(ty.0) || !self.ty_table.contains(ty.0) {
            return false;
        }
        let holds = match self.ty_table.get(ty.0) {
            TypeKind::Primitive(p) => p != LangPrimitive::Dynamic,
            TypeKind::Literal(_)
            | TypeKind::Record(_)
            | TypeKind::Tuple(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TemplateLiteral(_)
            | TypeKind::KeyOf(_)
            | TypeKind::TypePredicate { .. } => true,
            TypeKind::Union(list) | TypeKind::Intersection(list) => self
                .ty_table
                .get_list(list)
                .iter()
                .all(|id| self.equality_value_type(Type::resolved(*id), view, path)),
            TypeKind::Named(name, origin) | TypeKind::Generic(name, _, origin) => {
                let name = self.ty_table.names().try_resolve(name).unwrap_or_default();
                let origin = origin.and_then(|o| self.ty_table.names().try_resolve(o));
                if view.get_enum_members(name, origin).is_some() {
                    true
                } else {
                    view.resolve_type_alias(name, origin)
                        .is_some_and(|alias| self.equality_value_type(alias, view, path))
                }
            }
            TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Object(_)
            | TypeKind::Fn(_)
            | TypeKind::Typeof(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_) => false,
        };
        path.remove(&ty.0);
        holds
    }
}
