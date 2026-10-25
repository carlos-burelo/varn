use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::ExprId;
use varn_core::{BuiltinType, Diagnostic, ErrorCode, TypeKind};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn require_keyed_receiver(
        &mut self,
        rec: &mut Recorder,
        subject: ExprId,
        bind: &BindResult,
    ) {
        let ty = self.infer_type(rec, subject, bind);
        let ty = ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        let builtin = ty.builtin_of(&self.ty_table);
        let keyed = match self.ty_table.get(ty.0) {
            TypeKind::Array(_)
            | TypeKind::Tuple(_)
            | TypeKind::Primitive(_)
            | TypeKind::Literal(_) => false,
            TypeKind::Named(..) | TypeKind::Generic(..) => !matches!(
                builtin,
                Some(
                    BuiltinType::Array
                        | BuiltinType::Map
                        | BuiltinType::Set
                        | BuiltinType::Range
                        | BuiltinType::Bytes
                        | BuiltinType::Task
                        | BuiltinType::TaskHandle
                        | BuiltinType::Generator
                        | BuiltinType::AsyncGenerator
                )
            ),
            TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
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
            | TypeKind::TypePredicate { .. } => true,
        };
        if keyed || ty.is_dynamic() {
            return;
        }
        self.emit(
            Diagnostic::error(
                ErrorCode::NotIterable,
                format!(
                    "'{}' has no keys: `for ... in` walks the keys of an object, use `for ... of` for its elements",
                    ty.display(&self.ty_table, &bind.interner)
                ),
            )
            .with_range(self.ast_arena.expr(subject).range),
        );
    }
}
