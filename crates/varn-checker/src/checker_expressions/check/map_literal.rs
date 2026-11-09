use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::expr::PropKey;
use varn_core::ast::{ExprId, ObjectProp};
use varn_core::{BuiltinType, Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn expected_map(&mut self, expr: ExprId) -> Option<(Type, Type, Type)> {
        let expected = self.expected_for(expr)?;
        let map = expected.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        map.is_builtin(BuiltinType::Map, &self.ty_table).then(|| {
            (
                map,
                map.builtin_arg(0, &self.ty_table),
                map.builtin_arg(1, &self.ty_table),
            )
        })
    }

    pub(super) fn check_map_literal(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        properties: &[ObjectProp],
        (key_ty, value_ty): (Type, Type),
        bind: &BindResult,
    ) {
        rec.desugar.map_literals.insert(expr.index());
        let literal_range = self.ast_arena.expr(expr).range;
        for property in properties {
            match property {
                ObjectProp::Property { key, value, .. } => {
                    let (actual_key, key_range) = match key {
                        PropKey::Identifier(_) | PropKey::Str(_) => (Type::Str, literal_range),
                        PropKey::Int(_) => (Type::Int, literal_range),
                        PropKey::Computed(computed) => {
                            self.with_expected(Some(key_ty), |c| {
                                c.check_expr(rec, *computed, bind)
                            });
                            (
                                self.infer_type(rec, *computed, bind),
                                self.ast_arena.expr(*computed).range,
                            )
                        }
                    };
                    if !actual_key.is_unresolved()
                        && !self.types_compatible_cached(&key_ty, &actual_key, Some(bind))
                    {
                        let actual_s = actual_key.display(&self.ty_table, &bind.interner);
                        let expected_s = key_ty.display(&self.ty_table, &bind.interner);
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::TypeMismatch,
                                format!(
                                    "type mismatch: map key is '{actual_s}', expected '{expected_s}'"
                                ),
                            )
                            .with_range(key_range),
                        );
                    }
                    self.with_expected(Some(value_ty), |c| c.check_expr(rec, *value, bind));
                    let actual = self.infer_type(rec, *value, bind);
                    if !actual.is_unresolved()
                        && !self.value_fits(rec, &value_ty, &actual, Some(*value), Some(bind))
                    {
                        let actual_s = actual.display(&self.ty_table, &bind.interner);
                        let expected_s = value_ty.display(&self.ty_table, &bind.interner);
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::TypeMismatch,
                                format!(
                                    "type mismatch: map value is '{actual_s}', expected '{expected_s}'"
                                ),
                            )
                            .with_range(self.ast_arena.expr(*value).range),
                        );
                    }
                }
                ObjectProp::Method { .. } | ObjectProp::Spread { .. } => {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::TypeMismatch,
                            "a Map literal holds `key: value` entries only: add the others with `.set(..)`",
                        )
                        .with_range(literal_range),
                    );
                }
            }
        }
    }
}
