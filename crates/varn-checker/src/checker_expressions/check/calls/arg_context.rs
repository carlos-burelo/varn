use crate::checker::Checker;
use varn_core::ast::Arg;
use varn_core::TypeKind;
use varn_sem::bind::BindResult;
use varn_sem::types::{FunctionParam, Type};

impl<'r> Checker<'r> {
    pub(in super::super) fn check_call_args_with_context(
        &mut self,
        args: &[Arg],
        params: &[FunctionParam],
        bind: &BindResult,
    ) {
        for (i, arg) in args.iter().enumerate() {
            let param = if i < params.len() {
                Some(&params[i])
            } else if params.last().is_some_and(|p| p.is_rest) {
                params.last()
            } else {
                None
            };

            let expected = param.map(|p| {
                if p.is_rest {
                    match self.ty_table.get(p.ty) {
                        TypeKind::Array(inner) => Type::resolved(inner),
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
                        | TypeKind::Object(_)
                        | TypeKind::Typeof(_)
                        | TypeKind::KeyOf(_)
                        | TypeKind::IndexedAccess { .. }
                        | TypeKind::Mapped { .. }
                        | TypeKind::Conditional { .. }
                        | TypeKind::Infer(_)
                        | TypeKind::EnumVariant { .. }
                        | TypeKind::TypePredicate { .. } => Type::resolved(p.ty),
                    }
                } else {
                    Type::resolved(p.ty)
                }
            });
            match arg {
                Arg::Positional(e) | Arg::Spread(e) => {
                    let e = *e;
                    self.with_expected(expected, |c| c.check_expr(e, bind));
                }
                Arg::Named { value, .. } => self.check_expr(*value, bind),
            }
        }
    }
}
