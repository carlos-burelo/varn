use crate::binder::BindResult;
use crate::checker::Checker;
use crate::checker_generics::build_call_mapping;
use crate::generic_substitution::map_generics_cached;
use crate::types::{FunctionParam, FunctionType, Type};
use std::sync::Arc;
use varn_core::ast::{ExprId, ExprKind, Param};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

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
            ExprKind::IntLiteral { .. } | ExprKind::FloatLiteral { .. } | ExprKind::BigIntLiteral { .. } | ExprKind::DecimalLiteral { .. } | ExprKind::StrLiteral { .. } | ExprKind::CharLiteral { .. } | ExprKind::BoolLiteral { .. } | ExprKind::NullLiteral | ExprKind::RegexLiteral { .. } | ExprKind::Template { .. } | ExprKind::TaggedTemplate { .. } | ExprKind::Identifier { .. } | ExprKind::Missing | ExprKind::This | ExprKind::Super | ExprKind::Array { .. } | ExprKind::Object { .. } | ExprKind::Tuple { .. } | ExprKind::Record { .. } | ExprKind::Unary { .. } | ExprKind::Update { .. } | ExprKind::Binary { .. } | ExprKind::Logical { .. } | ExprKind::Assign { .. } | ExprKind::Conditional { .. } | ExprKind::Member { .. } | ExprKind::New { .. } | ExprKind::Function { .. } | ExprKind::Arrow { .. } | ExprKind::Sequence { .. } | ExprKind::Paren { .. } | ExprKind::Await { .. } | ExprKind::Spawn { .. } | ExprKind::Yield { .. } | ExprKind::Spread { .. } | ExprKind::Pipeline { .. } | ExprKind::Range { .. } | ExprKind::NonNull { .. } | ExprKind::Try { .. } | ExprKind::As { .. } | ExprKind::Satisfies { .. } | ExprKind::ClassExpr { .. } | ExprKind::Match { .. } | ExprKind::Is { .. } | ExprKind::With { .. } | ExprKind::MetaAccess { .. } => return Type::Dynamic,
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
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                );
            }
            return Type::named(
                class_name_str,
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            );
        }
        let TypeKind::Fn(fid) = callee_kind else {
            return Type::Dynamic;
        };
        let ft = self.ty_table.get_function(fid).clone();

        let mapping = build_call_mapping(callee, &type_args, &args, &ft, self, bind);
        let ret = map_generics_cached(self, &Type::resolved(ft.return_type), &mapping);

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
        let dynamic_context = self.expected_type.is_some_and(|t| t.is_dynamic());

        let ps: Vec<FunctionParam> = params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let name = crate::binder::pattern_lead_name(&p.pattern, &bind.interner);
                let mut ty = p
                    .type_ann
                    .as_ref()
                    .map(|m| self.resolve_type_node_cached(m, bind))
                    .or_else(|| {
                        expected_params
                            .get(i)
                            .map(|ep| Type::resolved(ep.ty))
                            .filter(|t| !t.is_dynamic())
                    })
                    .or_else(|| {
                        p.default.map(|d| {
                            let t = self.infer_type(d, bind);
                            crate::binder::widen_literal(t)
                        })
                    })
                    .unwrap_or_else(|| {
                        if dynamic_context {
                            return Type::Dynamic;
                        }
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::TypeAnnotationRequired,
                                format!("parameter '{name}' needs a type annotation: nothing in its context gives it one"),
                            )
                            .with_range(p.range),
                        );
                        Type::Error
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
                    self.symbol_types.insert(sym_id, Type::resolved(fp.ty));
                }
            }
        }

        let ret_ty = match return_type {
            Some(rt) => self.resolve_type_node_cached(rt, bind),
            None => super::arrow_body_return_type(body, self, bind),
        };

        if arrow_scope.is_some() {
            self.current_scope = saved_scope;
        }

        let ret_ty = crate::types::async_fn_return(
            ret_ty,
            is_async,
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            &bind.interner,
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
