use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::{Arg, ExprId, ExprKind};
use varn_core::TypeKind;
use varn_sem::bind::BindResult;
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn narrow_type_guard(
        &mut self,
        rec: &mut Recorder,
        callee: ExprId,
        args: &[Arg],
        bind: &BindResult,
        is_true_branch: bool,
        out: &mut Vec<(SymbolId, Type)>,
    ) {
        let arena = self.ast_arena;
        let callee_ty_raw = self.infer_type(rec, callee, bind);
        let callee_ty =
            callee_ty_raw.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        if let TypeKind::Fn(fid) = self.ty_table.get(callee_ty.0) {
            let ft = self.ty_table.get_function(fid).clone();
            if let TypeKind::TypePredicate {
                parameter_name,
                target_type,
            } = self.ty_table.get(ft.return_type)
            {
                let target_type = Type::resolved(target_type);
                let parameter_name_str = bind.interner.resolve(parameter_name);
                let arg_expr = if let Some(pos) = ft
                    .params
                    .iter()
                    .position(|p| p.name.as_deref() == Some(parameter_name_str))
                {
                    args.get(pos).and_then(|a| match a {
                        Arg::Positional(e) => Some(*e),
                        Arg::Spread(_) | Arg::Named { .. } => None,
                    })
                } else if args.len() == 1 {
                    match &args[0] {
                        Arg::Positional(e) => Some(*e),
                        Arg::Spread(_) | Arg::Named { .. } => None,
                    }
                } else {
                    None
                };

                if let Some(ExprKind::Identifier { name: arg_name }) =
                    arg_expr.map(|e| &arena.expr(e).kind)
                {
                    let scope = bind.scopes.get(self.current_scope);
                    if let Some(id) = scope.resolve(*arg_name, &bind.scopes) {
                        let original_ty = rec
                            .symbol_types
                            .get(&id)
                            .cloned()
                            .or_else(|| bind.arena.get(id).ty);
                        if is_true_branch {
                            if let Some(orig) = original_ty {
                                let orig_kind = self.ty_table.get(orig.0);
                                let target_kind = self.ty_table.get(target_type.0);
                                let matched: Vec<Type> = match orig_kind {
                                    TypeKind::Union(list) => self
                                        .ty_table
                                        .get_list(list)
                                        .to_vec()
                                        .into_iter()
                                        .map(Type::resolved)
                                        .filter(|m| {
                                            let m_kind = self.ty_table.get(m.0);
                                            match (m_kind, target_kind) {
                                                (TypeKind::Array(_), TypeKind::Array(_)) => true,
                                                _ => *m == target_type,
                                            }
                                        })
                                        .collect(),
                                    TypeKind::Primitive(_)
                                    | TypeKind::Builtin(_)
                                    | TypeKind::Literal(_)
                                    | TypeKind::This
                                    | TypeKind::Array(_)
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
                                    | TypeKind::TypePredicate { .. } => vec![target_type],
                                };
                                if !matched.is_empty() {
                                    let narrowed = if matched.len() == 1 {
                                        matched.into_iter().next().unwrap()
                                    } else {
                                        Type::union(
                                            matched,
                                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                                        )
                                    };
                                    out.push((id, narrowed));
                                } else {
                                    out.push((id, target_type));
                                }
                            } else {
                                out.push((id, target_type));
                            }
                        } else if let Some(original_ty) = original_ty {
                            let narrowed = original_ty.minus(
                                &target_type,
                                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                            );
                            if narrowed != original_ty {
                                out.push((id, narrowed));
                            }
                        }
                    }
                }
            }
        }
    }
}
