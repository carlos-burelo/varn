use crate::binder::BindResult;
use crate::checker::Checker;
use crate::checker_generics::{build_call_mapping, map_generics_cached};
use crate::types::{FunctionParam, FunctionType, Type};
use std::sync::Arc;
use varn_core::ast::{ExprId, ExprKind, Param};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

use super::collect_checked_return_types;

impl<'r> Checker<'r> {
    pub(super) fn infer_call_type(&mut self, expr: ExprId, bind: &BindResult) -> Type {
        let arena = self.ast_arena;
        let (callee, type_args, args) = match &arena.expr(expr).kind {
            ExprKind::Call {
                callee,
                type_args,
                args,
                ..
            } => (*callee, type_args.clone(), args.clone()),
            _ => return Type::Dynamic,
        };

        let callee_ty_raw = self.infer_type(callee, bind);
        let callee_ty =
            callee_ty_raw.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        let callee_kind = self.ty_table.get(callee_ty.0);

        if let TypeKind::Named(class_name, _) = callee_kind {
            let class_name_str = bind.interner.resolve(class_name).to_string();
            if !type_args.is_empty() {
                let resolved: Vec<Type> = type_args
                    .iter()
                    .map(|a| self.resolve_type_node_cached(a, bind))
                    .collect();
                return Type::generic(
                    class_name_str,
                    resolved,
                    self.resolver,
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                );
            }
            return Type::named(
                class_name_str,
                self.resolver,
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            );
        }
        let TypeKind::Fn(fid) = callee_kind else {
            return Type::Dynamic;
        };
        let ft = self.ty_table.get_function(fid).clone();

        let mapping = build_call_mapping(callee, &type_args, &args, &ft, self, bind);
        let ret = map_generics_cached(self, &Type(ft.return_type, false), &mapping);

        let ret_kind = self.ty_table.get(ret.0);
        let ret = if matches!(ret_kind, TypeKind::This) {
            if let ExprKind::Member { object, .. } = &arena.expr(callee).kind {
                let receiver_ty = self.infer_type(*object, bind);
                if !receiver_ty.is_dynamic() {
                    receiver_ty
                } else {
                    ret
                }
            } else {
                ret
            }
        } else {
            ret
        };

        // No `async` patch-up here: the callee's type already says `Task<R>`
        // if it is async — see `crate::types::async_fn_return`.
        ret
    }

    pub(super) fn infer_arrow_type(
        &mut self,
        _expr: ExprId,
        params: &[Param],
        return_type: &Option<varn_core::ast::TypeNode>,
        body: varn_core::ast::ArrowBody,
        is_async: bool,
        bind: &BindResult,
    ) -> Type {
        let expected_params: Vec<FunctionParam> = self
            .expected_type
            .and_then(|t| {
                if let TypeKind::Fn(fid) = self.ty_table.get(t.0) {
                    Some(self.ty_table.get_function(fid).params.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let ps: Vec<FunctionParam> = params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let name = crate::binder::pattern_lead_name(&p.pattern, &bind.interner);
                let mut ty = p
                    .type_ann
                    .as_ref()
                    .or(match &p.pattern {
                        varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                        _ => None,
                    })
                    .map(|m| self.resolve_type_node_cached(m, bind))
                    .or_else(|| {
                        expected_params
                            .get(i)
                            .map(|ep| Type(ep.ty, false))
                            .filter(|t| !t.is_dynamic())
                    })
                    .unwrap_or_else(|| {
                        if self.warn_implicit_dynamic && !name.is_empty() && name != "_" {
                            self.emit(
                                Diagnostic::hint(ErrorCode::TypeAnnotationRequired, format!("parameter '{name}' has no type annotation — inferred as 'dynamic'"))
                                    .with_range(*p.pattern.range()),
                            );
                        }
                        Type::Dynamic
                    });
                if p.is_rest {
                    let is_array = matches!(self.ty_table.get(ty.0), varn_core::TypeKind::Array(_));
                    if !is_array {
                        ty = Type::array(ty, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
                    }
                }
                FunctionParam {
                    name: Some(Arc::from(name)),
                    ty: ty.0,
                    optional: p.is_optional || p.default.is_some(),
                    is_rest: p.is_rest,
                }
            })
            .collect();

        // Seed the parameter types into the arrow's scope BEFORE inferring the
        // body's return type, so a block body that returns a parameter
        // (`(n) => { return n }`) resolves it to its contextual type instead of
        // `dynamic` (which would be dropped, collapsing the return type to
        // `void`). Mirrors `infer_arrow_with_context` used by generic inference,
        // keeping the two paths consistent.
        let arrow_scope =
            crate::checker_generics::find_arrow_scope(self.current_scope, params, bind);
        let saved_scope = self.current_scope;
        if let Some(scope_id) = arrow_scope {
            self.current_scope = scope_id;
            for (p, fp) in params.iter().zip(ps.iter()) {
                let name = crate::binder::pattern_lead_name(&p.pattern, &bind.interner);
                if name.is_empty() || name == "_" {
                    continue;
                }
                if let Some(sym_id) = bind
                    .interner
                    .get(name)
                    .and_then(|atom| bind.scopes.get(scope_id).resolve(atom, &bind.scopes))
                {
                    self.symbol_types.insert(sym_id, Type(fp.ty, false));
                }
            }
        }

        let ret_ty = if let Some(rt) = return_type {
            self.resolve_type_node_cached(rt, bind)
        } else {
            match body {
                varn_core::ast::ArrowBody::Expr(e) => self.infer_type(e, bind),
                varn_core::ast::ArrowBody::Block(block) => {
                    let return_tys = collect_checked_return_types(block, self, bind);
                    match return_tys.len() {
                        0 if !crate::checker::completion::can_complete_normally(
                            block,
                            self.ast_arena,
                        ) =>
                        {
                            Type::Never
                        }
                        0 => Type::Void,
                        1 => return_tys.into_iter().next().unwrap(),
                        _ => Type::union(
                            return_tys,
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        ),
                    }
                }
            }
        };

        if arrow_scope.is_some() {
            self.current_scope = saved_scope;
        }

        let ret_ty = crate::types::async_fn_return(
            ret_ty,
            is_async,
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            &bind.interner,
            Some(self.resolver),
        );
        Type::fn_(
            FunctionType {
                params: ps,
                return_type: ret_ty.0,
                is_arrow: true,
                type_params: vec![],
            },
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        )
    }
}
