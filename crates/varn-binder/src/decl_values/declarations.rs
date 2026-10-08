use super::super::type_inference::widen_literal;
use std::sync::Arc;
use varn_core::ast::{ExprKind, FunctionDecl, Pattern, VarKind, VariableDecl};
use varn_sem::bind::PendingEnrich;
use varn_sem::scope::ScopeKind;
use varn_sem::symbol::{Symbol, SymbolKind};
use varn_sem::types::{FunctionType, Type};

impl<'r> super::super::Binder<'r> {
    pub(crate) fn bind_variable(&mut self, v: &VariableDecl) {
        let sym_kind = match v.kind {
            VarKind::Const => SymbolKind::Const,
            VarKind::Let => SymbolKind::Let,
        };
        for d in &v.declarators {
            let line = d.range.start.line;
            let has_explicit_ann = d.type_ann.is_some();
            let ty = d
                .type_ann
                .as_ref()
                .map(|ann| self.resolve_type(ann))
                .or_else(|| {
                    d.init.map(|e| self.infer_expr_type_self(e)).map(|t| {
                        if sym_kind == SymbolKind::Let && !has_explicit_ann {
                            widen_literal(t)
                        } else {
                            t
                        }
                    })
                });

            let ty = if ty.is_none() {
                self.report_missing_variable_annotation(d);
                Some(Type::Error)
            } else {
                ty
            };
            let needs_enrich =
                !has_explicit_ann && (ty.is_none() || ty.as_ref().is_some_and(|t| t.is_dynamic()));
            self.bind_pattern(&d.id, sym_kind, line, v.doc.clone(), ty, has_explicit_ann);

            if let Pattern::Identifier { name, .. } = &d.id {
                if let Some(init) = d.init {
                    if let ExprKind::Object { properties } = &self.ast_arena.expr(init).kind {
                        let fields = self.collect_object_members(properties);
                        if !fields.is_empty() {
                            self.type_members.objects.insert(*name, fields);
                        }
                    }
                }
            }

            if let Some(init_expr) = d.init {
                if needs_enrich
                    && matches!(
                        &self.ast_arena.expr(init_expr).kind,
                        ExprKind::Call { .. }
                            | ExprKind::Await { .. }
                            | ExprKind::Member { .. }
                            | ExprKind::New { .. }
                            | ExprKind::Match { .. }
                            | ExprKind::Pipeline { .. }
                    )
                {
                    let scope = self.scopes.get(self.current);
                    let name_atom = if let Pattern::Identifier { name, .. } = &d.id {
                        Some(*name)
                    } else {
                        None
                    };
                    if let Some(sym_id) = name_atom.and_then(|n| scope.lookup(n)) {
                        self.pending_enrich.push(PendingEnrich::Var {
                            sym_id,
                            init: init_expr,
                        });
                    }
                } else if !has_explicit_ann
                    && matches!(&self.ast_arena.expr(init_expr).kind, ExprKind::Array { elements } if elements.is_empty())
                {
                    if let Pattern::Identifier { name, .. } = &d.id {
                        let scope = self.scopes.get(self.current);
                        if let Some(sym_id) = scope.lookup(*name) {
                            self.register_array_candidate(sym_id, *name);
                        }
                    }
                }
                self.bind_expr(init_expr);
            }
        }
    }

    pub(crate) fn bind_function(&mut self, f: &FunctionDecl) {
        let line = f.range.start.line;
        let params: Vec<varn_sem::types::FunctionParam> = f
            .params
            .iter()
            .map(|p| self.function_param(p, crate::ParamSite::Declared))
            .collect();

        let declared_ret = f.return_type.as_ref().map(|ann| self.resolve_type(ann));

        let ret = if f.modifiers.is_generator {
            varn_sem::types::generator_of(
                declared_ret.unwrap_or(Type::Dynamic),
                f.modifiers.is_async,
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            )
        } else {
            varn_sem::types::async_fn_return(
                declared_ret.unwrap_or(Type::Void),
                f.modifiers.is_async,
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            )
        };
        let fn_type = Type::fn_(
            FunctionType {
                params: params.clone(),
                return_type: ret.0,
                is_arrow: false,
                type_params: f
                    .type_params
                    .iter()
                    .map(|t| Arc::from(self.interner.resolve(t.name)))
                    .collect(),
            },
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        );

        let mut sym = Symbol::new(SymbolKind::Function, f.id, line).with_type(fn_type);
        sym.col = f.range.start.column + (f.id_offset - f.range.start.offset);
        sym.offset = f.id_offset;
        sym.has_explicit_type = f.return_type.is_some();
        sym.is_async = f.modifiers.is_async;
        sym.is_generator = f.modifiers.is_generator;
        sym.doc = f.doc.as_ref().map(|s| self.intern_local(s.as_str()));
        sym.type_params = f.type_params.iter().map(|t| t.name).collect();
        sym.type_param_constraints = f
            .type_params
            .iter()
            .map(|t| t.constraint.as_ref().map(|c| self.resolve_type(c)))
            .collect();
        let sym_id = self.define(f.id, sym);
        crate::decorator_attrs::record_decorators(self, sym_id, &f.decorators);

        if f.return_type.is_none() && !f.modifiers.is_declare && !f.modifiers.is_generator {
            self.pending_enrich.push(PendingEnrich::Fn {
                sym_id,
                body: f.body,
                is_async: f.modifiers.is_async,
            });
        }

        let child = self.scopes.child(ScopeKind::Function, self.current);
        let saved = self.current;
        self.current = child;

        self.bind_type_params(&f.type_params, line);

        for p in f.params.iter() {
            let ty = self.param_type(p, crate::ParamSite::Declared);

            self.bind_pattern(
                &p.pattern,
                SymbolKind::Parameter,
                line,
                f.doc.clone(),
                Some(ty),
                p.type_ann.is_some(),
            );
        }

        if !f.modifiers.is_declare {
            self.bind_stmt(f.body);
        }
        self.current = saved;

        let _ = sym_id;
    }
}
