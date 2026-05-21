use rustc_hash::FxHashSet;

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::checker_generics::{build_call_mapping, map_generics_cached};
use crate::types::{FunctionParam, Type};
use varn_core::ast::{Arg, Expr, ExprKind, TypeNode};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};

use super::members::extension_type_name;

impl Checker {
    pub(super) fn check_call_expr(
        &mut self,
        callee: &Expr,
        args: &[Arg],
        type_args: &[TypeNode],
        range: &SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(callee, bind);
        self.record_extension_call(callee, range, bind);

        let callee_ty = self.infer_type(callee, bind).non_nullified();

        if !matches!(
            &callee_ty.0,
            TypeKind::Fn(_)
                | TypeKind::Intrinsic(varn_core::TypeTag::Dynamic)
                | TypeKind::Named(_, _)
                | TypeKind::Generic(_, _, _)
        ) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotCallable,
                    format!("type '{callee_ty}' is not callable"),
                )
                .with_range(*range),
            );
        }

        let effective_callee_ty = if let TypeKind::Fn(ft) = &callee_ty.0 {
            let mapping = build_call_mapping(callee, type_args, args, ft, self, bind);
            map_generics_cached(self, &callee_ty, &mapping)
        } else {
            callee_ty.clone()
        };

        if let ExprKind::Member { property, .. } = &callee.kind {
            self.record_type(property.range.start.offset, effective_callee_ty.clone());
        } else {
            self.record_type(callee.range.start.offset, effective_callee_ty.clone());
        }

        let params_for_context: Vec<FunctionParam> = if let TypeKind::Fn(ft) = &callee_ty.0 {
            ft.params.clone()
        } else {
            vec![]
        };
        self.check_call_args_with_context(args, &params_for_context, bind);

        if let TypeKind::Fn(crate::types::FunctionType { params, .. }) = &effective_callee_ty.0 {
            self.validate_call_arguments(args, params, range, bind);
        }

        self.check_type_arg_constraints(callee, type_args, range, bind);
    }

    fn record_extension_call(&mut self, callee: &Expr, range: &SourceRange, bind: &BindResult) {
        let ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } = &callee.kind
        else {
            return;
        };
        let ExprKind::Identifier { name: method_name } = &property.kind else {
            return;
        };
        let obj_ty = self.infer_type(object, bind).non_nullified();
        let Some(tn) = extension_type_name(&obj_ty) else {
            return;
        };
        let Some(method_map) = bind.extensions.methods.get(tn.as_ref()) else {
            return;
        };
        if let Some(mangled) = method_map.get(method_name.as_ref()) {
            self.extension_calls
                .insert(range.start.offset, mangled.clone());
        }
    }

    fn check_call_args_with_context(
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
                    match &p.ty.0 {
                        TypeKind::Array(inner) => *inner.clone(),
                        _ => p.ty.clone(),
                    }
                } else {
                    p.ty.clone()
                }
            });
            match arg {
                Arg::Positional(e) | Arg::Spread(e) => {
                    self.with_expected(expected, |c| c.check_expr(e, bind));
                }
                Arg::Named { value, .. } => self.check_expr(value, bind),
            }
        }
    }

    fn validate_call_arguments(
        &mut self,
        args: &[Arg],
        params: &[FunctionParam],
        range: &SourceRange,
        bind: &BindResult,
    ) {
        let (call_info, extra_diags) = analyze_call_args(args, range);
        self.diagnostics.extend(extra_diags);
        let has_named = !call_info.named_labels.is_empty();

        let has_error = if has_named {
            self.validate_named_call_arguments(args, params, &call_info.named_labels, range, bind)
        } else {
            self.validate_positional_call_arguments(args, params, range, bind)
        };

        if !has_error && !call_info.has_spread {
            let required_count = params.iter().filter(|p| !p.optional && !p.is_rest).count();
            let has_rest = params.last().is_some_and(|p| p.is_rest);

            if args.len() < required_count {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::TypeMismatch,
                        format!(
                            "expected at least {} arguments, but got {}",
                            required_count,
                            args.len()
                        ),
                    )
                    .with_range(*range),
                );
            } else if args.len() > params.len() && !has_rest {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::TypeMismatch,
                        format!(
                            "expected at most {} arguments, but got {}",
                            params.len(),
                            args.len()
                        ),
                    )
                    .with_range(*range),
                );
            }
        }
    }

    fn validate_named_call_arguments(
        &mut self,
        args: &[Arg],
        params: &[FunctionParam],
        named_labels: &FxHashSet<&str>,
        range: &SourceRange,
        bind: &BindResult,
    ) -> bool {
        let mut positional_param_idx = 0usize;
        let mut has_error = false;

        let mut mapping = vec![None; params.len()];

        for arg in args {
            let (label_opt, arg_ty) = match arg {
                Arg::Named { label, value } => (Some(label.as_str()), self.infer_type(value, bind)),
                Arg::Positional(e) => (None, self.infer_type(e, bind)),
                Arg::Spread(e) => {
                    let arg_ty = self.infer_type(e, bind);
                    if !arg_ty.is_dynamic() && !matches!(arg_ty.0, TypeKind::Array(_)) {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::TypeMismatch,
                                format!("spread argument must be an array, got '{arg_ty}'"),
                            )
                            .with_range(*range),
                        );
                        has_error = true;
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
                    (Some(&p.ty), Some(i))
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
                let ty = params.get(positional_param_idx).map(|p| &p.ty);
                let idx = if ty.is_some() {
                    Some(positional_param_idx)
                } else {
                    None
                };
                positional_param_idx += 1;
                (ty, idx)
            };

            if let Some(idx) = param_idx {
                let arg_idx = args.iter().position(|a| std::ptr::eq(a, arg)).unwrap();
                mapping[idx] = Some(arg_idx);
            }

            if let Some(param_ty) = param_ty {
                if !self.types_compatible_cached(param_ty, &arg_ty, Some(bind)) {
                    let msg = if let Some(lbl) = label_opt {
                        format!(
                            "named argument '{lbl}' of type '{arg_ty}' is not assignable to parameter of type '{param_ty}'"
                        )
                    } else {
                        format!(
                            "argument of type '{arg_ty}' is not assignable to parameter of type '{param_ty}'"
                        )
                    };
                    self.emit(Diagnostic::error(ErrorCode::TypeMismatch, msg).with_range(*range));
                    has_error = true;
                }
            }
        }

        self.call_mappings.insert(range.start.offset, mapping);

        has_error
    }

    fn validate_positional_call_arguments(
        &mut self,
        args: &[Arg],
        params: &[FunctionParam],
        range: &SourceRange,
        bind: &BindResult,
    ) -> bool {
        for (i, arg) in args.iter().enumerate() {
            let (arg_ty, spread_inner) = match self.infer_arg_type(arg, bind) {
                Ok(value) => value,
                Err(arg_ty) => {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::TypeMismatch,
                            format!("spread argument must be an array, got '{arg_ty}'"),
                        )
                        .with_range(*range),
                    );
                    return true;
                }
            };

            let param = if i < params.len() {
                Some(&params[i])
            } else if params.last().is_some_and(|p| p.is_rest) {
                params.last()
            } else {
                None
            };

            if let Some(param) = param {
                let effective_arg_ty = spread_inner.as_ref().unwrap_or(&arg_ty);
                let param_ty = compatible_param_type(param, spread_inner.as_ref());

                let param_accepts_array = matches!(param_ty.0, TypeKind::Array(_))
                    || matches!(&param_ty.0, TypeKind::Union(ms) if ms.iter().any(|m| matches!(m.0, TypeKind::Array(_))));
                let is_empty_array_arg = effective_arg_ty.is_dynamic()
                    && matches!(
                        match arg {
                            Arg::Positional(e) | Arg::Named { value: e, .. } | Arg::Spread(e) => &e.kind,
                        },
                        ExprKind::Array { elements } if elements.is_empty()
                    )
                    && param_accepts_array;

                if !is_empty_array_arg
                    && !self.types_compatible_cached(param_ty, effective_arg_ty, Some(bind))
                {
                    self.emit(
                        Diagnostic::error(ErrorCode::TypeMismatch, format!(
                            "argument of type '{effective_arg_ty}' is not assignable to parameter of type '{param_ty}'"
                        ))
                        .with_range(*range),
                    );
                    return true;
                }
            }
        }

        false
    }

    fn infer_arg_type(
        &mut self,
        arg: &Arg,
        bind: &BindResult,
    ) -> Result<(Type, Option<Type>), Type> {
        match arg {
            Arg::Positional(expr) | Arg::Named { value: expr, .. } => {
                Ok((self.infer_type(expr, bind), None))
            }
            Arg::Spread(expr) => {
                let arg_ty = self.infer_type(expr, bind);
                let spread_inner = match &arg_ty.0 {
                    TypeKind::Array(inner) => Some(inner.as_ref().clone()),
                    _ => None,
                };
                match spread_inner {
                    Some(inner) => Ok((arg_ty, Some(inner))),
                    None if arg_ty.is_dynamic() => Ok((arg_ty, None)),
                    None => Err(arg_ty),
                }
            }
        }
    }

    fn check_type_arg_constraints(
        &mut self,
        callee: &Expr,
        type_args: &[TypeNode],
        range: &SourceRange,
        bind: &BindResult,
    ) {
        if type_args.is_empty() {
            return;
        }
        let ExprKind::Identifier { name: fn_name } = &callee.kind else {
            return;
        };
        let Some(fn_sym) = resolve_function_symbol(fn_name.as_ref(), self.current_scope, bind)
        else {
            return;
        };

        let resolved: Vec<Type> = type_args
            .iter()
            .map(|a| self.resolve_type_node_cached(a, bind))
            .collect();
        for (i, constraint) in fn_sym.type_param_constraints.iter().enumerate() {
            if let (Some(ct), Some(supplied)) = (constraint, resolved.get(i)) {
                if !self.types_compatible_cached(ct, supplied, Some(bind)) {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::ConstraintViolation,
                            format!("type '{supplied}' does not satisfy constraint '{ct}'"),
                        )
                        .with_range(*range),
                    );
                }
            }
        }
    }
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

fn resolve_function_symbol<'a>(
    name: &str,
    current_scope: crate::scope::ScopeId,
    bind: &'a BindResult,
) -> Option<&'a crate::symbol::Symbol> {
    let current = bind.scopes.get(current_scope);
    if let Some(id) = current.resolve(name, &bind.scopes) {
        let sym = bind.arena.get(id);
        if matches!(sym.kind, crate::symbol::SymbolKind::Function) {
            return Some(sym);
        }
    }

    let global = bind.scopes.get(bind.global_scope);
    if let Some(id) = global.resolve(name, &bind.scopes) {
        let sym = bind.arena.get(id);
        if matches!(sym.kind, crate::symbol::SymbolKind::Function) {
            return Some(sym);
        }
    }

    None
}

fn compatible_param_type<'a>(param: &'a FunctionParam, spread_inner: Option<&'a Type>) -> &'a Type {
    if let Some(inner) = spread_inner {
        if param.is_rest {
            if let TypeKind::Array(expected_inner) = &param.ty.0 {
                expected_inner.as_ref()
            } else {
                &param.ty
            }
        } else {
            inner
        }
    } else if param.is_rest {
        if let TypeKind::Array(inner) = &param.ty.0 {
            inner
        } else {
            &param.ty
        }
    } else {
        &param.ty
    }
}
