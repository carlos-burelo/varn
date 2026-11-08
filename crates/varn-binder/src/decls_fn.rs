use super::Binder;
use std::sync::Arc;
use varn_core::ast::{Arg, ExprId, MatchPattern, Param, StmtId};
use varn_sem::types::Type;

impl<'r> Binder<'r> {
    pub(super) fn bind_inline_function(
        &mut self,
        type_params: &[varn_core::ast::TypeParam],
        params: &[varn_core::ast::Param],
        _return_type: Option<&varn_core::ast::TypeNode>,
        body: StmtId,
        range: &varn_core::SourceRange,
    ) {
        use varn_sem::scope::ScopeKind;

        let line = range.start.line;

        let child = self.scopes.child(ScopeKind::Function, self.current);
        let saved = self.current;
        self.current = child;

        self.bind_type_params(type_params, line);
        self.bind_function_params(params, line);

        self.bind_stmt(body);
        self.current = saved;
    }

    pub(super) fn bind_inline_function_expr(
        &mut self,
        params: &[Param],
        body: ExprId,
        range: &varn_core::SourceRange,
    ) {
        use varn_sem::scope::ScopeKind;

        let child = self.scopes.child(ScopeKind::Function, self.current);
        let saved = self.current;
        self.current = child;
        self.bind_function_params(params, range.start.line);
        self.bind_expr(body);
        self.current = saved;
    }

    pub(super) fn bind_function_params(&mut self, params: &[Param], line: u32) {
        use varn_sem::symbol::SymbolKind;

        for p in params {
            let ty = self.param_binding_type(p, super::binding_types::ParamSite::Closure);

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
        use varn_sem::symbol::{Symbol, SymbolKind};
        use varn_sem::types::Type;

        for tp in type_params {
            let name_rc: Arc<str> = Arc::from(self.interner.resolve(tp.name));
            let tp_ty = Type::named(name_rc, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
            let mut sym = Symbol::new(SymbolKind::TypeParameter, tp.name, line).with_type(tp_ty);
            sym.type_param_constraints =
                vec![tp.constraint.as_ref().map(|bound| self.resolve_type(bound))];
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
    bind_typed_pattern_vars(b, pattern, None);
}

fn define_pattern_var(
    b: &mut Binder,
    name: varn_core::Atom,
    range: Option<varn_core::SourceRange>,
    ty: Type,
) {
    use varn_sem::symbol::{Symbol, SymbolKind};
    if b.interner.resolve(name) == varn_core::TokenKind::Placeholder.as_str() {
        return;
    }
    let line = range.map_or(0, |r| r.start.line);
    let mut sym = Symbol::new(SymbolKind::Let, name, line).with_type(ty);
    if let Some(r) = range {
        sym.col = r.start.column;
        sym.offset = r.start.offset;
    }
    b.define(name, sym);
}

fn bind_typed_pattern_vars(b: &mut Binder, pattern: &MatchPattern, ty: Option<Type>) {
    match pattern {
        MatchPattern::Wildcard => {}
        MatchPattern::Literal(expr) => b.bind_expr(*expr),
        MatchPattern::Identifier(name) => {
            define_pattern_var(b, *name, None, ty.unwrap_or(Type::Error));
        }
        MatchPattern::Tuple(items) => {
            for item in items {
                bind_typed_pattern_vars(b, item, None);
            }
        }
        MatchPattern::Array { elements, rest } => {
            for element in elements {
                bind_typed_pattern_vars(b, element, None);
            }
            if let varn_core::ast::SequenceRest::Bind(name) = rest {
                define_pattern_var(b, *name, None, Type::Error);
            }
        }
        MatchPattern::Record { fields, .. } => {
            for field in fields {
                bind_field_vars(b, field, Type::Error);
            }
        }
        MatchPattern::EnumVariant {
            variant_name,
            payload,
            ..
        } => {
            let field_types = b
                .sum_variant_fields
                .get(b.interner.resolve(*variant_name))
                .cloned()
                .unwrap_or_default();
            match payload {
                varn_core::ast::VariantPayload::Positional(items) => {
                    for (i, item) in items.iter().enumerate() {
                        let field_ty = field_types.get(i).map(|(_, t)| *t);
                        bind_typed_pattern_vars(b, item, field_ty);
                    }
                }
                varn_core::ast::VariantPayload::Named { fields, .. } => {
                    for field in fields {
                        let field_name = b.interner.resolve(field.name);
                        let field_ty = field_types
                            .iter()
                            .find(|(n, _)| n.as_ref() == field_name)
                            .map(|(_, t)| *t)
                            .unwrap_or(Type::Error);
                        bind_field_vars(b, field, field_ty);
                    }
                }
            }
        }
    }
}

fn bind_field_vars(b: &mut Binder, field: &varn_core::ast::MatchField, ty: Type) {
    match &field.pattern {
        Some(sub) => bind_typed_pattern_vars(b, sub, Some(ty)),
        None => define_pattern_var(b, field.name, Some(field.range), ty),
    }
}
