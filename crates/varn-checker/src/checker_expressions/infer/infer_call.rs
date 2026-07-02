use crate::binder::BindResult;
use crate::checker::Checker;
use crate::checker_generics::{build_call_mapping, map_generics_cached};
use crate::types::{FunctionParam, FunctionType, Type};
use std::rc::Rc;
use varn_core::ast::{Expr, ExprKind, Param};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

use super::collect_checked_return_types;

impl Checker {
    pub(super) fn infer_call_type(&mut self, expr: &Expr, bind: &BindResult) -> Type {
        let (callee, type_args, args) = match &expr.kind {
            ExprKind::Call {
                callee,
                type_args,
                args,
                ..
            } => (callee, type_args, args),
            _ => return Type::Dynamic,
        };

        let callee_ty = self.infer_type(callee, bind).non_nullified();

        if let TypeKind::Named(class_name, _) = &callee_ty.0 {
            if !type_args.is_empty() {
                let resolved: Vec<Type> = type_args
                    .iter()
                    .map(|a| self.resolve_type_node_cached(a, bind))
                    .collect();
                return Type::generic(class_name.clone(), resolved);
            }
            return Type::named(class_name.clone());
        }
        let TypeKind::Fn(ft) = &callee_ty.0 else {
            return Type::Dynamic;
        };

        let mapping = build_call_mapping(callee, type_args, args, ft, self, bind);
        let ret = map_generics_cached(self, &ft.return_type, &mapping);

        let ret = if matches!(ret.0, TypeKind::This) {
            if let ExprKind::Member { object, .. } = &callee.kind {
                let receiver_ty = self.infer_type(object, bind);
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

        let is_async_callee = if let ExprKind::Identifier { name } = &callee.kind {
            let scope = bind.scopes.get(self.current_scope);
            scope
                .resolve(name, &bind.scopes)
                .map(|id| bind.arena.get(id).is_async)
                .unwrap_or(false)
        } else if self.extension_calls.contains_key(&expr.range.start.offset) {
            self.extension_calls
                .get(&expr.range.start.offset)
                .and_then(|mangled| {
                    let scope = bind.scopes.get(bind.global_scope);
                    scope.resolve(mangled, &bind.scopes)
                })
                .map(|id| bind.arena.get(id).is_async)
                .unwrap_or(false)
        } else {
            false
        };

        if is_async_callee
            && !matches!(&ret.0, TypeKind::Generic(n, _, _) if n.as_ref() == varn_core::IntrinsicType::Task.as_str())
            && !ret.is_dynamic()
            && ret != Type::Void
        {
            Type::generic(
                varn_core::IntrinsicType::Task.as_str().to_owned(),
                vec![ret],
            )
        } else {
            ret
        }
    }

    pub(super) fn infer_arrow_type(
        &mut self,
        _expr: &Expr,
        params: &[Param],
        return_type: &Option<varn_core::ast::TypeNode>,
        body: &varn_core::ast::ArrowBody,
        bind: &BindResult,
    ) -> Type {
        let expected_params: Vec<FunctionParam> = self
            .expected_type
            .as_ref()
            .and_then(|t| {
                if let TypeKind::Fn(ft) = &t.0 {
                    Some(ft.params.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let ps: Vec<FunctionParam> = params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let name = crate::binder::pattern_lead_name(&p.pattern);
                let mut ty = p
                    .type_ann
                    .as_ref()
                    .or(match &p.pattern {
                        varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                        _ => None,
                    })
                    .map(|m| self.resolve_type_node_cached(m, bind))
                    .or_else(|| {
                        expected_params.get(i).map(|ep| ep.ty.clone()).filter(|t| !t.is_dynamic())
                    })
                    .unwrap_or_else(|| {
                        if self.warn_implicit_dynamic && !name.is_empty() && name != "_" {
                            self.emit(
                                Diagnostic::hint(ErrorCode::TypeAnnotationRequired, format!("parameter '{name}' has no type annotation — inferred as 'dynamic'"))
                                    .with_range(p.pattern.range().clone()),
                            );
                        }
                        Type::Dynamic
                    });
                if p.is_rest && !matches!(ty.0, varn_core::TypeKind::Array(_)) {
                    ty = Type::array(ty);
                }
                FunctionParam {
                    name: Some(Rc::from(name)),
                    ty,
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
                let name = crate::binder::pattern_lead_name(&p.pattern);
                if name.is_empty() || name == "_" {
                    continue;
                }
                if let Some(sym_id) = bind.scopes.get(scope_id).resolve(&name, &bind.scopes) {
                    self.symbol_types.insert(sym_id, fp.ty.clone());
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
                        0 => Type::Void,
                        1 => return_tys.into_iter().next().unwrap(),
                        _ => Type::union(return_tys),
                    }
                }
            }
        };

        if arrow_scope.is_some() {
            self.current_scope = saved_scope;
        }

        Type::fn_(FunctionType {
            params: ps,
            return_type: Box::new(ret_ty),
            is_arrow: true,
            type_params: vec![],
        })
    }
}
