use rustc_hash::FxHashSet;

use super::param_types::compatible_param_type;
use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::{Arg, ExprKind};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::types::{FunctionParam, Type};

impl<'r> Checker<'r> {
    pub(in super::super) fn validate_call_arguments(
        &mut self,
        rec: &mut Recorder,
        args: &[Arg],
        params: &[FunctionParam],
        range: &SourceRange,
        call_id: varn_core::ast::AstId,
        bind: &BindResult,
    ) {
        rec.desugar.call_params.insert(
            call_id,
            params
                .iter()
                .map(|p| varn_sem::output::CheckedParam {
                    ty: varn_sem::types::Type::resolved(p.ty),
                    optional: p.optional,
                    is_rest: p.is_rest,
                })
                .collect(),
        );
        let (call_info, extra_diags) = analyze_call_args(args, range);
        self.diagnostics.extend(extra_diags);
        let has_named = !call_info.named_labels.is_empty();

        let (has_error, supplied) = if has_named {
            let has_error = self.validate_named_call_arguments(
                rec,
                args,
                params,
                &call_info.named_labels,
                range,
                call_id,
                bind,
            );
            (has_error, (!call_info.has_spread).then_some(args.len()))
        } else {
            self.validate_positional_call_arguments(rec, args, params, range, bind)
        };

        if let (false, Some(supplied)) = (has_error, supplied) {
            let required_count = params.iter().filter(|p| !p.optional && !p.is_rest).count();
            let has_rest = params.last().is_some_and(|p| p.is_rest);

            if supplied < required_count {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::TypeMismatch,
                        format!(
                            "expected at least {} arguments, but got {}",
                            required_count, supplied
                        ),
                    )
                    .with_range(*range),
                );
            } else if supplied > params.len() && !has_rest {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::TypeMismatch,
                        format!(
                            "expected at most {} arguments, but got {}",
                            params.len(),
                            supplied
                        ),
                    )
                    .with_range(*range),
                );
            }
        }
    }

    fn validate_positional_call_arguments(
        &mut self,
        rec: &mut Recorder,
        args: &[Arg],
        params: &[FunctionParam],
        range: &SourceRange,
        bind: &BindResult,
    ) -> (bool, Option<usize>) {
        let mut supplied: Vec<(Type, Option<Type>, varn_core::ast::ExprId)> = Vec::new();
        let mut known_count = true;
        for arg in args {
            let expr = match arg {
                Arg::Positional(e) | Arg::Spread(e) => *e,
                Arg::Named { value, .. } => *value,
            };
            match self.infer_arg_type(rec, arg, bind) {
                Ok(Supplied::One(ty)) => supplied.push((ty, None, expr)),
                Ok(Supplied::Each(types)) => {
                    supplied.extend(types.into_iter().map(|ty| (ty, None, expr)));
                }
                Ok(Supplied::Some(ty, inner)) => {
                    known_count = false;
                    supplied.push((ty, inner, expr));
                }
                Err(arg_ty) => {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::TypeMismatch,
                            format!(
                                "spread argument must be an array or a tuple, got '{}'",
                                arg_ty.display(&self.ty_table, &bind.interner)
                            ),
                        )
                        .with_range(*range),
                    );
                    return (true, None);
                }
            }
        }
        let count = known_count.then_some(supplied.len());

        for (i, (arg_ty, spread_inner, expr)) in supplied.into_iter().enumerate() {
            let param = if i < params.len() {
                Some(&params[i])
            } else if params.last().is_some_and(|p| p.is_rest) {
                params.last()
            } else {
                None
            };

            if let Some(param) = param {
                let effective_arg_ty = spread_inner.unwrap_or(arg_ty);
                let param_ty = compatible_param_type(param, spread_inner, &self.ty_table);
                let param_ty = self.accepted_param_type(param, param_ty);

                let param_kind = self.ty_table.get(param_ty.0);
                let param_accepts_array = matches!(param_kind, TypeKind::Array(_))
                    || matches!(param_kind, TypeKind::Union(list) if self
                        .ty_table
                        .get_list(list)
                        .iter()
                        .any(|m| matches!(self.ty_table.get(*m), TypeKind::Array(_))));
                let is_empty_array_arg = effective_arg_ty.is_dynamic()
                    && matches!(
                        &self.ast_arena.expr(expr).kind,
                        ExprKind::Array { elements } if elements.is_empty()
                    )
                    && param_accepts_array;

                if !is_empty_array_arg
                    && !self.value_fits(rec, &param_ty, &effective_arg_ty, Some(expr), Some(bind))
                {
                    let effective_arg_ty_s =
                        effective_arg_ty.display(&self.ty_table, &bind.interner);
                    let param_ty_s = param_ty.display(&self.ty_table, &bind.interner);
                    self.emit(
                        Diagnostic::error(ErrorCode::TypeMismatch, format!(
                            "argument of type '{effective_arg_ty_s}' is not assignable to parameter of type '{param_ty_s}'"
                        ))
                        .with_range(*range),
                    );
                    return (true, None);
                }
            }
        }

        (false, count)
    }

    pub(in super::super) fn infer_arg_type(
        &mut self,
        rec: &mut Recorder,
        arg: &Arg,
        bind: &BindResult,
    ) -> Result<Supplied, Type> {
        match arg {
            Arg::Positional(expr) | Arg::Named { value: expr, .. } => {
                Ok(Supplied::One(self.infer_type(rec, *expr, bind)))
            }
            Arg::Spread(expr) => {
                let arg_ty = self.infer_type(rec, *expr, bind);
                let spread_inner = match self.ty_table.get(arg_ty.0) {
                    TypeKind::Array(inner) => Some(Type::resolved(inner)),
                    TypeKind::Tuple(elements) => {
                        return Ok(Supplied::Each(
                            self.ty_table
                                .get_list(elements)
                                .iter()
                                .map(|el| Type::resolved(*el))
                                .collect(),
                        ));
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
                match spread_inner {
                    Some(inner) => Ok(Supplied::Some(arg_ty, Some(inner))),
                    None if arg_ty.is_dynamic() => Ok(Supplied::Some(arg_ty, None)),
                    None => Err(arg_ty),
                }
            }
        }
    }
}

pub(in super::super) enum Supplied {
    One(Type),
    Each(Vec<Type>),
    Some(Type, Option<Type>),
}

struct CallArgAnalysis<'a> {
    named_labels: FxHashSet<&'a str>,
    has_spread: bool,
}

fn analyze_call_args<'a>(
    args: &'a [Arg],
    range: &SourceRange,
) -> (CallArgAnalysis<'a>, varn_core::DiagnosticBag) {
    let mut named_labels = FxHashSet::default();
    let mut has_spread = false;
    let mut seen_named_arg = false;
    let mut reported_named_order_error = false;
    let mut extra_diags = varn_core::DiagnosticBag::new();

    for arg in args {
        match arg {
            Arg::Named { label, .. } => {
                seen_named_arg = true;
                named_labels.insert(label.as_str());
            }
            Arg::Positional(_) if seen_named_arg && !reported_named_order_error => {
                extra_diags.error(
                    ErrorCode::InvalidExpression,
                    "positional argument cannot follow a named argument",
                    *range,
                );
                reported_named_order_error = true;
            }
            Arg::Spread(_) => has_spread = true,
            Arg::Positional(_) => {}
        }
    }

    (
        CallArgAnalysis {
            named_labels,
            has_spread,
        },
        extra_diags,
    )
}
