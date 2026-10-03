use super::Binder;
use crate::types::Type;
use std::sync::Arc;
use varn_core::ast::{Arg, ExprId, MatchPattern, Param, StmtId};

impl<'r> Binder<'r> {
    pub(super) fn bind_inline_function(
        &mut self,
        type_params: &[varn_core::ast::TypeParam],
        params: &[varn_core::ast::Param],
        _return_type: Option<&varn_core::ast::TypeNode>,
        body: StmtId,
        range: &varn_core::SourceRange,
    ) {
        use crate::scope::ScopeKind;

        let line = range.start.line;

        self.escape_all_open_array_candidates();

        let child = self.scopes.child(ScopeKind::Function, self.current);
        let saved = self.current;
        self.current = child;

        self.bind_type_params(type_params, line);
        self.bind_function_params(params, line);

        self.bind_stmt(body);
        self.finalize_array_watch(child);
        self.current = saved;
    }

    pub(super) fn bind_inline_function_expr(
        &mut self,
        params: &[Param],
        body: ExprId,
        range: &varn_core::SourceRange,
    ) {
        use crate::scope::ScopeKind;

        self.escape_all_open_array_candidates();

        let child = self.scopes.child(ScopeKind::Function, self.current);
        let saved = self.current;
        self.current = child;
        self.bind_function_params(params, range.start.line);
        self.bind_expr(body);
        self.finalize_array_watch(child);
        self.current = saved;
    }

    pub(super) fn bind_function_params(&mut self, params: &[Param], line: u32) {
        use crate::symbol::SymbolKind;

        for p in params {
            let ty = self.param_type(p, super::binding_types::ParamSite::Closure);

            self.bind_pattern(
                &p.pattern,
                SymbolKind::Parameter,
                line,
                None,
                Some(ty),
                p.type_ann.is_some(),
            );

            if let Some(default_value) = p.default {
                self.bind_expr(default_value);
            }
        }
    }

    pub(super) fn bind_type_params(
        &mut self,
        type_params: &[varn_core::ast::TypeParam],
        line: u32,
    ) {
        use crate::symbol::{Symbol, SymbolKind};
        use crate::types::Type;

        for tp in type_params {
            let name_rc: Arc<str> = Arc::from(self.interner.resolve(tp.name));
            let tp_ty = Type::named(name_rc, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
            let mut sym = Symbol::new(SymbolKind::TypeParameter, tp.name, line).with_type(tp_ty);
            sym.col = tp.range.start.column;
            sym.offset = tp.range.start.offset;
            self.define(tp.name, sym);
        }
    }

    pub(super) fn bind_args(&mut self, args: &[Arg]) {
        for arg in args {
            match arg {
                Arg::Positional(expr) | Arg::Spread(expr) => self.bind_expr(*expr),
                Arg::Named { value, .. } => self.bind_expr(*value),
            }
        }
    }
}

pub(super) fn bind_match_pattern_vars(b: &mut Binder, pattern: &MatchPattern) {
    use crate::symbol::Symbol;
    use crate::symbol::SymbolKind;
    match pattern {
        MatchPattern::Wildcard | MatchPattern::Type { .. } => {}
        MatchPattern::Literal(expr) => b.bind_expr(*expr),
        MatchPattern::EnumVariant {
            variant_name,
            bindings,
            ..
        } => {
            let field_types = b
                .sum_variant_fields
                .get(b.interner.resolve(*variant_name))
                .cloned();
            for (i, binding) in bindings.iter().enumerate() {
                if b.interner.resolve(binding.name) != "_" {
                    let ty = field_types
                        .as_ref()
                        .and_then(|fields| fields.get(i))
                        .map(|(_, t)| *t)
                        .unwrap_or(Type::Dynamic);
                    let mut sym =
                        Symbol::new(SymbolKind::Let, binding.name, binding.range.start.line)
                            .with_type(ty);
                    sym.col = binding.range.start.column;
                    sym.offset = binding.range.start.offset;
                    b.define(binding.name, sym);
                }
            }
        }

        MatchPattern::Identifier(name) => {
            if b.interner.resolve(*name) != "_" {
                let sym = Symbol::new(SymbolKind::Let, *name, 0).with_type(Type::Dynamic);
                b.define(*name, sym);
            }
        }
        MatchPattern::Record { fields, .. } => {
            let variant_name = fields.first().and_then(|(key, sub)| {
                if b.interner.resolve(*key) == varn_core::MemberKey::Variant.as_str() {
                    if let Some(MatchPattern::Identifier(n)) = sub {
                        return Some(*n);
                    }
                }
                None
            });

            if let Some(vname) = variant_name {
                let field_types: Vec<(Arc<str>, Type)> = b
                    .sum_variant_fields
                    .get(b.interner.resolve(vname))
                    .cloned()
                    .unwrap_or_default();

                for (field_key, sub_pat) in fields.iter().skip(1) {
                    let binding_name = match sub_pat {
                        Some(MatchPattern::Identifier(n)) => *n,
                        _ => *field_key,
                    };
                    if b.interner.resolve(binding_name) == "_" {
                        continue;
                    }

                    let field_key_str = b.interner.resolve(*field_key);
                    let ty = field_types
                        .iter()
                        .find(|(fname, _)| fname.as_ref() == field_key_str)
                        .map(|(_, t)| *t)
                        .unwrap_or(Type::Dynamic);
                    let sym = Symbol::new(SymbolKind::Let, binding_name, 0).with_type(ty);
                    b.define(binding_name, sym);

                    if let Some(sub) = sub_pat {
                        if !matches!(sub, MatchPattern::Identifier(_)) {
                            bind_match_pattern_vars(b, sub);
                        }
                    }
                }
            } else {
                for (field_name, sub_pat) in fields {
                    let binding_name = match sub_pat {
                        Some(MatchPattern::Identifier(n)) => *n,
                        _ => *field_name,
                    };
                    if b.interner.resolve(binding_name) != "_" {
                        let sym =
                            Symbol::new(SymbolKind::Let, binding_name, 0).with_type(Type::Dynamic);
                        b.define(binding_name, sym);
                    }
                    if let Some(sub) = sub_pat {
                        if !matches!(sub, MatchPattern::Identifier(_)) {
                            bind_match_pattern_vars(b, sub);
                        }
                    }
                }
            }
        }
        MatchPattern::Sequence(pats) => {
            for p in pats {
                bind_match_pattern_vars(b, p);
            }
        }
    }
}
