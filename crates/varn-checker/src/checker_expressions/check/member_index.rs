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
        let builtin = check_ty.builtin_of(&self.ty_table);
        let key_expected = match check_kind {
            TypeKind::Generic(_, args, _) if builtin == Some(varn_core::BuiltinType::Map) => {
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
            TypeKind::Array(_) => Some(Type::Int),
            TypeKind::Named(..) if builtin == Some(varn_core::BuiltinType::Bytes) => {
                Some(Type::Int)
            }
            TypeKind::Generic(..) | TypeKind::Named(..)
                if builtin == Some(varn_core::BuiltinType::Range) =>
            {
                Some(Type::Int)
            }
            TypeKind::Tuple(slots) => {
                let arity = self.ty_table.get_list(slots).len();
                if let ExprKind::IntLiteral { value, .. } = &self.ast_arena.expr(property).kind
                    && usize::try_from(*value).map_or(true, |at| at >= arity)
                {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::TupleIndexOutOfBounds,
                            format!(
                                "tuple index {value} is out of bounds: '{}' has {arity} elements",
                                check_ty.display(&self.ty_table, &bind.interner)
                            ),
                        )
                        .with_range(property_range),
                    );
                }
                Some(Type::Int)
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
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
            let is_range_slice = (matches!(
                check_kind,
                TypeKind::Array(_) | TypeKind::Primitive(varn_core::LangPrimitive::Str)
            ) || builtin == Some(varn_core::BuiltinType::Bytes))
                && actual_k.is_range(&self.ty_table);
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
        } else if matches!(
            check_kind,
            TypeKind::Object(_) | TypeKind::Named(..) | TypeKind::Generic(..)
        ) {
            self.check_named_index(rec, object, property, &check_ty, property_range, bind);
        } else {
            self.check_expr(rec, property, bind);
        }
    }

    fn check_named_index(
        &mut self,
        rec: &mut Recorder,
        object: ExprId,
        property: ExprId,
        owner: &Type,
        property_range: SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(rec, property, bind);
        let member = match &self.ast_arena.expr(property).kind {
            ExprKind::StrLiteral { value } if self.member_exists_cached(owner, value, bind) => {
                Some(value.clone())
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
            | ExprKind::Identifier { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::ImportMeta(_)
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
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
            | ExprKind::MetaAccess { .. } => None,
        };
        match member {
            Some(name) => {
                self.check_member_side(object, owner, &name, &property_range, bind);
                self.check_member_access_rights(owner, &name, &property_range, bind);
            }
            None => self.emit(
                Diagnostic::error(
                    ErrorCode::NotIndexable,
                    format!(
                        "'{}' has no index signature: read a member with '.', declare an index signature or use a Map",
                        owner.display(&self.ty_table, &bind.interner)
                    ),
                )
                .with_range(property_range),
            ),
        }
    }
}
