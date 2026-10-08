use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use crate::checker_generics::build_call_mapping;
use crate::generic_substitution::map_generics_cached;
use varn_core::ast::{Arg, ExprId, ExprKind, TypeNode};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::types::{FunctionParam, Type};

impl<'r> Checker<'r> {
    pub(in super::super) fn check_call_expr(
        &mut self,
        rec: &mut Recorder,
        callee: ExprId,
        args: &[Arg],
        type_args: &[TypeNode],
        range: &SourceRange,
        call_id: varn_core::ast::AstId,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        self.check_expr(rec, callee, bind);
        self.record_extension_call(rec, callee, range, bind);

        let callee_ty_raw = self.infer_type(rec, callee, bind);
        let callee_ty =
            callee_ty_raw.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        let callee_kind = self.ty_table.get(callee_ty.0);

        let callee_sid: Option<usize> = match &arena.expr(callee).kind {
            ExprKind::Identifier { name } => bind
                .scopes
                .get(self.current_scope)
                .resolve(*name, &bind.scopes),
            ExprKind::Member {
                object,
                property,
                computed: false,
                ..
            } => {
                if let ExprKind::Identifier { name: prop } = &arena.expr(*property).kind {
                    let prop_name = bind.interner.resolve(*prop);
                    let obj_ty = self.infer_type(rec, *object, bind);
                    self.find_member_info(&obj_ty, prop_name, bind)
                        .and_then(|(_, sid)| {
                            sid.filter(|s| {
                                *s < bind.arena.len()
                                    && bind.interner.get(prop_name) == Some(bind.arena.get(*s).name)
                            })
                        })
                } else {
                    None
                }
            }
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => None,
        };
        self.check_pure_callee(callee_sid, *range, bind);
        self.check_capability_callee(callee_sid, *range, bind);

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
            let mapping = build_call_mapping(callee, type_args, args, &ft, self, rec, bind);
            map_generics_cached(self, &callee_ty, &mapping)
        } else {
            callee_ty
        };

        if let ExprKind::Member { property, .. } = &arena.expr(callee).kind {
            let property = *property;
            self.record_type(
                rec,
                arena.expr(property).range.start.offset,
                effective_callee_ty,
            );
        } else {
            self.record_type(
                rec,
                arena.expr(callee).range.start.offset,
                effective_callee_ty,
            );
        }

        let effective_kind = self.ty_table.get(effective_callee_ty.0);
        let params_for_context: Vec<FunctionParam> = if let TypeKind::Fn(fid) = effective_kind {
            self.ty_table.get_function(fid).params.clone()
        } else {
            vec![]
        };
        self.check_call_args_with_context(rec, args, &params_for_context, bind);

        if let TypeKind::Fn(fid) = effective_kind {
            let params = self.ty_table.get_function(fid).params.clone();
            self.validate_call_arguments(rec, args, &params, range, call_id, bind);
        }

        if rec.enabled {
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
                    ExprKind::IntLiteral { .. }
                    | ExprKind::FloatLiteral { .. }
                    | ExprKind::BigIntLiteral { .. }
                    | ExprKind::DecimalLiteral { .. }
                    | ExprKind::StrLiteral { .. }
                    | ExprKind::CharLiteral { .. }
                    | ExprKind::BoolLiteral { .. }
                    | ExprKind::NullLiteral
                    | ExprKind::RegexLiteral { .. }
                    | ExprKind::Template { .. }
                    | ExprKind::TaggedTemplate { .. }
                    | ExprKind::Missing
                    | ExprKind::This
                    | ExprKind::Super
                    | ExprKind::Array { .. }
                    | ExprKind::Object { .. }
                    | ExprKind::Tuple { .. }
                    | ExprKind::Record { .. }
                    | ExprKind::Unary { .. }
                    | ExprKind::Update { .. }
                    | ExprKind::Binary { .. }
                    | ExprKind::Logical { .. }
                    | ExprKind::Assign { .. }
                    | ExprKind::Conditional { .. }
                    | ExprKind::Call { .. }
                    | ExprKind::New { .. }
                    | ExprKind::Function { .. }
                    | ExprKind::Arrow { .. }
                    | ExprKind::Sequence { .. }
                    | ExprKind::Paren { .. }
                    | ExprKind::Await { .. }
                    | ExprKind::Spawn { .. }
                    | ExprKind::Yield { .. }
                    | ExprKind::Spread { .. }
                    | ExprKind::Pipeline { .. }
                    | ExprKind::Range { .. }
                    | ExprKind::NonNull { .. }
                    | ExprKind::Try { .. }
                    | ExprKind::As { .. }
                    | ExprKind::Satisfies { .. }
                    | ExprKind::ClassExpr { .. }
                    | ExprKind::Match { .. }
                    | ExprKind::Is { .. }
                    | ExprKind::With { .. }
                    | ExprKind::MetaAccess { .. } => None,
                };
                let params = ft
                    .params
                    .iter()
                    .map(|p| varn_sem::semantic_info::CallParamInfo {
                        name: p.name.clone(),
                        ty: Type::resolved(p.ty),
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

                let call_res = varn_sem::semantic_info::CallResolution {
                    callee_name,
                    params,
                    return_ty: Type::resolved(ft.return_type),
                    arg_to_param_map,
                };
                rec.call_resolutions
                    .insert(range.start.offset, call_res.clone());
                rec.call_resolutions
                    .insert(arena.expr(callee).range.start.offset, call_res);
            }
        }

        self.check_type_arg_constraints(callee, type_args, range, bind);
    }
}
