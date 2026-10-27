use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::ExprId;
use varn_core::{Diagnostic, ErrorCode, LangPrimitive, TypeKind, TypeLiteral};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn require_condition(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        bind: &BindResult,
    ) {
        let ty = self.infer_type(rec, expr, bind);
        if !self.never_false(ty, bind) {
            return;
        }
        let shown = ty.display(&self.ty_table, &bind.interner).to_string();
        self.emit(
            Diagnostic::error(
                ErrorCode::ConditionNeverFalse,
                format!(
                    "'{shown}' is never null or false, so it cannot be a condition: compare it explicitly"
                ),
            )
            .with_range(self.ast_arena.expr(expr).range),
        );
    }

    pub(crate) fn require_nullable(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        what: &str,
        bind: &BindResult,
    ) {
        let ty = self.infer_type(rec, expr, bind);
        let is_bool = matches!(
            self.ty_table.get(ty.0),
            TypeKind::Primitive(LangPrimitive::Bool) | TypeKind::Literal(TypeLiteral::Bool(_))
        );
        if !is_bool && !self.never_false(ty, bind) {
            return;
        }
        let shown = ty.display(&self.ty_table, &bind.interner).to_string();
        self.emit(
            Diagnostic::error(
                ErrorCode::NeverNull,
                format!("'{shown}' is never null, so {what} does nothing here"),
            )
            .with_range(self.ast_arena.expr(expr).range),
        );
    }

    pub(crate) fn never_false(&self, ty: Type, bind: &BindResult) -> bool {
        match self.ty_table.get(ty.0) {
            TypeKind::Primitive(
                LangPrimitive::Int
                | LangPrimitive::Float
                | LangPrimitive::BigInt
                | LangPrimitive::Decimal
                | LangPrimitive::Char
                | LangPrimitive::Str,
            ) => true,
            TypeKind::Primitive(
                LangPrimitive::Null
                | LangPrimitive::Bool
                | LangPrimitive::Void
                | LangPrimitive::Never
                | LangPrimitive::Dynamic,
            ) => false,
            TypeKind::Literal(TypeLiteral::Bool(_)) => false,
            TypeKind::Literal(TypeLiteral::Int(_) | TypeLiteral::Str(_) | TypeLiteral::Char(_)) => {
                true
            }
            TypeKind::Array(_)
            | TypeKind::Tuple(_)
            | TypeKind::Fn(_)
            | TypeKind::Object(_)
            | TypeKind::Record(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::EnumVariant { .. } => true,
            TypeKind::Union(list) => self
                .ty_table
                .get_list(list)
                .iter()
                .all(|member| self.never_false(Type::resolved(*member), bind)),
            TypeKind::Named(..) | TypeKind::Generic(..) => self.is_class_instance(ty, bind),
            TypeKind::This
            | TypeKind::Intersection(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::TypePredicate { .. } => false,
        }
    }
}
