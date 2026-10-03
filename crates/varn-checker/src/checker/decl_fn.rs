use super::Checker;
use crate::binder::BindResult;
use crate::types::Type;
use std::sync::Arc;

impl<'r> Checker<'r> {
    pub(super) fn check_function_decl(
        &mut self,
        f: &varn_core::ast::FunctionDecl,
        bind: &BindResult,
    ) {
        let saved_expected = self.expected_return_type.take();
        self.expected_return_type = f.return_type.as_ref().map(|rt| {
            let ty = self.resolve_type_node_cached(rt, bind);
            if f.modifiers.is_async {
                crate::types::awaited(&ty, &self.ty_table, &bind.interner)
            } else {
                ty
            }
        });

        let saved_scope = self.current_scope;
        let next_scope = self.next_child_scope(bind);
        if let Some(fn_scope) = next_scope {
            self.current_scope = fn_scope;
            self.record_scope_span(f.range.start.offset, f.range.end.offset, fn_scope);
        }
        let mut injected_tps = Vec::new();
        for tp in &f.type_params {
            let tp_name: Arc<str> = Arc::from(bind.interner.resolve(tp.name));
            self.active_type_params.insert(tp_name.clone());
            injected_tps.push(tp_name);
        }

        let is_gen = f.modifiers.is_generator;
        let old_yields = if is_gen {
            self.yielded_types.replace(Vec::new())
        } else {
            None
        };

        let saved_in_function = self.in_function;
        let saved_loop_depth = self.loop_depth;
        let saved_switch_depth = self.switch_depth;
        self.in_function = true;
        self.loop_depth = 0;
        self.switch_depth = 0;

        self.check_stmt(f.body, bind);

        self.in_function = saved_in_function;
        self.loop_depth = saved_loop_depth;
        self.switch_depth = saved_switch_depth;

        if is_gen {
            let yields = self.yielded_types.take().unwrap_or_default();
            if f.return_type.is_none() {
                let inferred_yield = if yields.is_empty() {
                    Type::Void
                } else {
                    Type::union(yields, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
                };
                let scope = bind.scopes.get(saved_scope);
                if let Some(sym_id) = scope.resolve(f.id, &bind.scopes) {
                    if let Some(fn_ty) = self
                        .symbol_types
                        .get(&sym_id)
                        .cloned()
                        .or_else(|| bind.arena.get(sym_id).ty)
                    {
                        if let varn_core::TypeKind::Fn(fid) = self.ty_table.get(fn_ty.0) {
                            let new_ret = crate::types::generator_of(
                                inferred_yield,
                                f.modifiers.is_async,
                                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                                Some(self.resolver),
                            );
                            let mut ft = self.ty_table.get_function(fid).clone();
                            ft.return_type = new_ret.0;
                            let new_fn_ty =
                                Type::fn_(ft, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
                            self.symbol_types.insert(sym_id, new_fn_ty);
                            self.record_type_with_symbol(f.id_offset, new_fn_ty, sym_id);
                        }
                    }
                }
            }
            self.yielded_types = old_yields;
        }

        self.current_scope = saved_scope;
        self.expected_return_type = saved_expected;
        for tp in &injected_tps {
            self.active_type_params.remove(tp.as_ref());
        }
    }
}
