use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::types::{ObjectTypeMember, Type};

impl<'r> Checker<'r> {
    pub(super) fn check_member_index(
        &mut self,
        rec: &mut Recorder,
        object: ExprId,
        property: ExprId,
        property_range: SourceRange,
        bind: &BindResult,
    ) {
        if matches!(self.ast_arena.expr(property).kind, ExprKind::Range { .. }) {
            self.check_expr(rec, property, bind);
            return;
        }
        let obj_ty = self.infer_type(rec, object, bind);
        let check_ty = obj_ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        let check_kind = self.ty_table.get(check_ty.0);
        let key_expected = match check_kind {
            TypeKind::Generic(name, args, _)
                if bind.interner.get(varn_core::BuiltinType::Map.name()) == Some(name) =>
            {
                let arg_ids = self.ty_table.get_list(args).to_vec();
                if arg_ids.len() == 2 {
                    Some(Type::resolved(arg_ids[0]))
                } else {
                    None
                }
            }
            TypeKind::Object(mid) => {
                self.ty_table
                    .get_object_members(mid)
                    .iter()
                    .find_map(|m| match m {
                        ObjectTypeMember::Index { key_ty, .. } => Some(Type::resolved(*key_ty)),
                        ObjectTypeMember::Property { .. }
                        | ObjectTypeMember::Method { .. }
                        | ObjectTypeMember::Callable { .. } => None,
                    })
            }
            TypeKind::Array(_) | TypeKind::Builtin(varn_core::BuiltinType::Bytes) => {
                Some(Type::Int)
            }
            TypeKind::Primitive(_)
            | TypeKind::Builtin(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => None,
        };
        if let Some(expected_k) = key_expected {
            self.with_expected(Some(expected_k), |c| c.check_expr(rec, property, bind));
            let actual_k = self.infer_type(rec, property, bind);
            let is_range_slice = matches!(
                check_kind,
                TypeKind::Array(_)
                    | TypeKind::Primitive(varn_core::LangPrimitive::Str)
                    | TypeKind::Builtin(varn_core::BuiltinType::Bytes)
            ) && actual_k.is_range(&self.ty_table, &bind.interner);
            if !actual_k.is_dynamic()
                && !is_range_slice
                && !self.types_compatible_cached(&expected_k, &actual_k, Some(bind))
            {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::TypeMismatch,
                        format!(
                            "type mismatch: index key is '{}', expected '{}'",
                            actual_k.display(&self.ty_table, &bind.interner),
                            expected_k.display(&self.ty_table, &bind.interner)
                        ),
                    )
                    .with_range(property_range),
                );
            }
        } else {
            self.check_expr(rec, property, bind);
        }
        return;
    }
}
