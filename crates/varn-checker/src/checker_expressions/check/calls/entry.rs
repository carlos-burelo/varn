use crate::binder::BindResult;
use crate::checker::Checker;
use crate::checker_generics::{build_call_mapping, map_generics_cached};
use crate::types::{FunctionParam, Type};
use varn_core::ast::{Arg, ExprId, ExprKind, TypeNode};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};

impl<'r> Checker<'r> {
    pub(in super::super) fn check_call_expr(
        &mut self,
        callee: ExprId,
        args: &[Arg],
        type_args: &[TypeNode],
        range: &SourceRange,
        call_id: varn_core::ast::AstId,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        self.check_expr(callee, bind);
        self.record_extension_call(callee, range, bind);

        let callee_ty_raw = self.infer_type(callee, bind);
        let callee_ty =
            callee_ty_raw.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        let callee_kind = self.ty_table.get(callee_ty.0);

        if !matches!(
            callee_kind,
            TypeKind::Fn(_)
                | TypeKind::Primitive(varn_core::LangPrimitive::Dynamic)
                | TypeKind::Named(_, _)
                | TypeKind::Generic(_, _, _)
        ) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotCallable,
                    format!(
                        "type '{}' is not callable",
                        callee_ty.display(&self.ty_table, &bind.interner)
                    ),
                )
                .with_range(*range),
            );
        }

        let effective_callee_ty = if let TypeKind::Fn(fid) = callee_kind {
            let ft = self.ty_table.get_function(fid).clone();
            let mapping = build_call_mapping(callee, type_args, args, &ft, self, bind);
            map_generics_cached(self, &callee_ty, &mapping)
        } else {
            callee_ty
        };

        if let ExprKind::Member { property, .. } = &arena.expr(callee).kind {
            let property = *property;
            self.record_type(arena.expr(property).range.start.offset, effective_callee_ty);
        } else {
            self.record_type(arena.expr(callee).range.start.offset, effective_callee_ty);
        }

        let effective_kind = self.ty_table.get(effective_callee_ty.0);
        let params_for_context: Vec<FunctionParam> = if let TypeKind::Fn(fid) = effective_kind {
            self.ty_table.get_function(fid).params.clone()
        } else {
            vec![]
        };
        self.check_call_args_with_context(args, &params_for_context, bind);

        if let TypeKind::Fn(fid) = effective_kind {
            let params = self.ty_table.get_function(fid).params.clone();
            self.validate_call_arguments(args, &params, range, call_id, bind);
        }

        if self.record_expr_types {
            if let TypeKind::Fn(fid) = effective_kind {
                let ft = self.ty_table.get_function(fid).clone();
                let callee_name = match &arena.expr(callee).kind {
                    ExprKind::Identifier { name } => {
                        Some(std::sync::Arc::from(bind.interner.resolve(*name)))
                    }
                    ExprKind::Member { property, .. } => {
                        if let ExprKind::Identifier { name } = &arena.expr(*property).kind {
                            Some(std::sync::Arc::from(bind.interner.resolve(*name)))
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let params = ft
                    .params
                    .iter()
                    .map(|p| crate::semantic_info::CallParamInfo {
                        name: p.name.clone(),
                        ty: Type(p.ty, false),
                        optional: p.optional,
                        is_rest: p.is_rest,
                    })
                    .collect();

                let mut arg_to_param_map = Vec::with_capacity(args.len());
                for (i, arg) in args.iter().enumerate() {
                    match arg {
                        Arg::Named { label, .. } => {
                            if let Some(pos) = ft
                                .params
                                .iter()
                                .position(|p| p.name.as_deref() == Some(label.as_str()))
                            {
                                arg_to_param_map.push(pos);
                            } else {
                                arg_to_param_map.push(i);
                            }
                        }
                        Arg::Positional(_) | Arg::Spread(_) => {
                            arg_to_param_map.push(i);
                        }
                    }
                }

                let call_res = crate::semantic_info::CallResolution {
                    callee_name,
                    params,
                    return_ty: Type(ft.return_type, false),
                    arg_to_param_map,
                };
                self.call_resolutions
                    .insert(range.start.offset, call_res.clone());
                self.call_resolutions
                    .insert(arena.expr(callee).range.start.offset, call_res);
            }
        }

        self.check_type_arg_constraints(callee, type_args, range, bind);
    }
}
