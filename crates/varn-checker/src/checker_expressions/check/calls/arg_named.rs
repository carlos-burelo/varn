use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use rustc_hash::FxHashSet;
use varn_core::ast::Arg;
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::types::{FunctionParam, Type};

impl<'r> Checker<'r> {
    pub(super) fn validate_named_call_arguments(
        &mut self,
        rec: &mut Recorder,
        args: &[Arg],
        params: &[FunctionParam],
        named_labels: &FxHashSet<&str>,
        range: &SourceRange,
        call_id: varn_core::ast::AstId,
        bind: &BindResult,
    ) -> bool {
        let mut positional_param_idx = 0usize;
        let mut has_error = false;

        let mut mapping = vec![None; params.len()];

        for arg in args {
            let (label_opt, arg_ty) = match arg {
                Arg::Named { label, value } => {
                    let expected = params
                        .iter()
                        .find(|p| p.name.as_deref() == Some(label.as_str()))
                        .map(|p| Type::resolved(p.ty));
                    (
                        Some(label.as_str()),
                        self.with_expected(expected, |c| c.infer_type(rec, *value, bind)),
                    )
                }
                Arg::Positional(e) => (None, self.infer_type(rec, *e, bind)),
                Arg::Spread(e) => {
                    let arg_ty = self.infer_type(rec, *e, bind);
                    if !arg_ty.is_dynamic()
                        && !matches!(
                            self.ty_table.get(arg_ty.0),
                            TypeKind::Array(_) | TypeKind::Tuple(_)
                        )
                    {
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
                        has_error = true;
                    }
                    if params.last().is_some_and(|p| p.is_rest)
                        && let Some(idx) = params.len().checked_sub(1)
                        && mapping[idx].is_none()
                    {
                        let arg_idx = args.iter().position(|a| std::ptr::eq(a, arg)).unwrap();
                        mapping[idx] = Some(arg_idx);
                    }
                    continue;
                }
            };

            let (param_ty, param_idx) = if let Some(lbl) = label_opt {
                if let Some((i, p)) = params
                    .iter()
                    .enumerate()
                    .find(|(_, p)| p.name.as_deref() == Some(lbl))
                {
                    (
                        Some(self.accepted_param_type(p, Type::resolved(p.ty))),
                        Some(i),
                    )
                } else {
                    (None, None)
                }
            } else {
                while positional_param_idx < params.len()
                    && params[positional_param_idx]
                        .name
                        .as_deref()
                        .is_some_and(|name| named_labels.contains(name))
                {
                    positional_param_idx += 1;
                }
                let ty = params
                    .get(positional_param_idx)
                    .map(|p| self.accepted_param_type(p, Type::resolved(p.ty)));
                let idx = if ty.is_some() {
                    Some(positional_param_idx)
                } else {
                    None
                };
                positional_param_idx += 1;
                (ty, idx)
            };

            let taken = param_idx.is_some_and(|idx| mapping[idx].is_some());
            if let Some(problem) =
                super::param_types::named_slot_problem(label_opt, param_idx, taken)
            {
                self.emit(Diagnostic::error(ErrorCode::TypeMismatch, problem).with_range(*range));
                has_error = true;
            }
            if let Some(idx) = param_idx {
                let arg_idx = args.iter().position(|a| std::ptr::eq(a, arg)).unwrap();
                mapping[idx] = Some(arg_idx);
            }

            if let Some(param_ty) = param_ty {
                let expr = match arg {
                    Arg::Positional(e) | Arg::Spread(e) => *e,
                    Arg::Named { value, .. } => *value,
                };
                if !self.value_fits(rec, &param_ty, &arg_ty, Some(expr), Some(bind)) {
                    let arg_ty_s = arg_ty.display(&self.ty_table, &bind.interner);
                    let param_ty_s = param_ty.display(&self.ty_table, &bind.interner);
                    let msg = if let Some(lbl) = label_opt {
                        format!(
                            "named argument '{lbl}' of type '{arg_ty_s}' is not assignable to parameter of type '{param_ty_s}'"
                        )
                    } else {
                        format!(
                            "argument of type '{arg_ty_s}' is not assignable to parameter of type '{param_ty_s}'"
                        )
                    };
                    self.emit(Diagnostic::error(ErrorCode::TypeMismatch, msg).with_range(*range));
                    has_error = true;
                }
            }
        }

        rec.call_mappings.insert(call_id, mapping);

        has_error
    }
}
