use crate::symbol::{Symbol, SymbolId};
use crate::types::Type;
use std::rc::Rc;
use varn_core::ast::pattern::MatchPattern;
use varn_core::ast::{
    Arg, ArrayEl, ArrowBody, Decl, Expr, ExprKind, MatchBody, ObjectProp, Param, TemplatePart,
};
use varn_core::{Diagnostic, ErrorCode};

impl super::Binder {
    pub(super) fn bind_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Variable(v) => self.bind_variable(v),
            Decl::Function(f) => self.bind_function(f),
            Decl::Class(c) => self.bind_class(c),
            Decl::Interface(i) => self.bind_interface(i),
            Decl::TypeAlias(t) => self.bind_type_alias(t),
            Decl::Enum(e) => self.bind_enum(e),
            Decl::Namespace(n) => self.bind_namespace(n),
            Decl::Struct(s) => self.bind_struct(s),
            Decl::Extension(e) => self.bind_extension(e),
            Decl::Import(i) => self.bind_import(i),
            Decl::Export(e) => self.bind_export(e),
            Decl::SumType(t) => self.bind_sum_type(t),
        }
    }

    pub(super) fn define(&mut self, name: String, sym: Symbol) -> SymbolId {
        let scope = self.scopes.get(self.current);
        if let Some(existing_id) = scope.lookup(&name) {
            let existing_sym = self.arena.get(existing_id);
            let existing_from_extern = existing_sym.origin_module.is_some()
                || (existing_sym.line == 0 && existing_sym.full_range.start.line == 0);
            let new_is_import = sym.origin_module.is_some();
            if existing_sym.kind == crate::symbol::SymbolKind::EnumMember
                || sym.kind == crate::symbol::SymbolKind::EnumMember
            {
                let id = self.arena.push(sym);
                let rc_name: Rc<str> = name.clone().into();
                self.scopes.get_mut(self.current).define(rc_name, id);
                return id;
            }
            if existing_from_extern && new_is_import {
                let id = self.arena.push(sym);
                let rc_name: Rc<str> = name.clone().into();
                self.scopes.get_mut(self.current).define(rc_name, id);
                return id;
            }
            let msg = format!("duplicate declaration of '{}'", name);

            let mut range = sym.full_range;
            if range.start.line == 0 && range.end.line == 0 {
                range.start.line = sym.line;
                range.end.line = sym.line;
            }

            let mut existing_range = existing_sym.full_range;
            if existing_range.start.line == 0 && existing_range.end.line == 0 {
                existing_range.start.line = existing_sym.line;
                existing_range.end.line = existing_sym.line;
            }

            let diag = Diagnostic::error(ErrorCode::DuplicateDeclaration, msg)
                .with_file(self.source_file.clone())
                .with_range(range)
                .with_related(
                    "original declaration here",
                    self.source_file.clone(),
                    existing_range,
                );
            self.diagnostics.push(diag);
        }

        let id = self.arena.push(sym);
        let rc_name: Rc<str> = name.into();
        self.scopes.get_mut(self.current).define(rc_name, id);
        id
    }

    pub(super) fn bind_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Arrow {
                params,
                return_type,
                body,
                ..
            } => match body.as_ref() {
                ArrowBody::Block(stmt) => {
                    self.bind_inline_function(&[], params, return_type.as_ref(), stmt, &expr.range);
                }
                ArrowBody::Expr(e) => {
                    self.bind_inline_function_expr(params, e, &expr.range);
                }
            },
            ExprKind::Function {
                params,
                return_type,
                body,
                ..
            } => {
                self.bind_inline_function(&[], params, return_type.as_ref(), body, &expr.range);
            }
            ExprKind::As { expression, .. }
            | ExprKind::Is { expression, .. }
            | ExprKind::Satisfies { expression, .. } => self.bind_expr(expression),
            ExprKind::Await { argument } | ExprKind::Spawn { argument } => self.bind_expr(argument),
            ExprKind::Try { expression } => self.bind_expr(expression),
            ExprKind::Yield { argument, .. } => {
                if let Some(arg) = argument {
                    self.bind_expr(arg);
                }
            }
            ExprKind::Unary { operand, .. } => self.bind_expr(operand),
            ExprKind::Binary { left, right, .. } | ExprKind::Logical { left, right, .. } => {
                self.bind_expr(left);
                self.bind_expr(right);
            }
            ExprKind::Assign { target, value, .. } => {
                self.bind_expr(target);
                self.bind_expr(value);
            }
            ExprKind::Call { callee, args, .. } => {
                self.bind_expr(callee);
                self.bind_args(args);
            }
            ExprKind::New { callee, args, .. } => {
                self.bind_expr(callee);
                self.bind_args(args);
            }
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.bind_expr(test);
                self.bind_expr(consequent);
                self.bind_expr(alternate);
            }
            ExprKind::Member {
                object,
                property,
                computed,
                ..
            } => {
                self.bind_expr(object);
                if *computed {
                    self.bind_expr(property);
                }
            }
            ExprKind::Paren { expression } => self.bind_expr(expression),
            ExprKind::NonNull { expression } => self.bind_expr(expression),
            ExprKind::Array { elements } => {
                for el in elements {
                    match el {
                        ArrayEl::Expr(e) => self.bind_expr(e),
                        ArrayEl::Spread(e) => self.bind_expr(e),
                        ArrayEl::Hole => {}
                    }
                }
            }
            ExprKind::Object { properties } => {
                for prop in properties {
                    match prop {
                        ObjectProp::Property { value, .. } => self.bind_expr(value),
                        ObjectProp::Method {
                            params,
                            return_type,
                            body,
                            range,
                            ..
                        } => {
                            self.bind_inline_function(
                                &[],
                                params,
                                return_type.as_ref(),
                                body,
                                range,
                            );
                        }
                        ObjectProp::Getter { body, .. } => {
                            self.bind_stmt(body);
                        }
                        ObjectProp::Setter { body, .. } => {
                            self.bind_stmt(body);
                        }
                        ObjectProp::Spread { argument, .. } => self.bind_expr(argument),
                    }
                }
            }
            ExprKind::Template { parts } => {
                for p in parts {
                    if let TemplatePart::Interpolation(e) = p {
                        self.bind_expr(e);
                    }
                }
            }
            ExprKind::Sequence { expressions } => {
                for e in expressions {
                    self.bind_expr(e);
                }
            }
            ExprKind::ClassExpr { declaration } => {
                self.bind_class(declaration);
            }
            ExprKind::Match { subject, cases } => {
                self.bind_expr(subject);
                for case in cases {
                    use crate::scope::ScopeKind;

                    let child = self.scopes.child(ScopeKind::Block, self.current);
                    let saved = self.current;
                    self.current = child;

                    bind_match_pattern_vars(self, &case.pattern);

                    if let Some(g) = &case.guard {
                        self.bind_expr(g);
                    }
                    match &case.body {
                        MatchBody::Expr(e) => self.bind_expr(e),
                        MatchBody::Block(stmt) => self.bind_stmt(stmt),
                    }
                    self.current = saved;
                }
            }
            ExprKind::Update { operand, .. } => self.bind_expr(operand),
            ExprKind::Spread { argument } => self.bind_expr(argument),
            ExprKind::Pipeline { left, right } => {
                self.bind_expr(left);
                self.bind_expr(right);
            }
            ExprKind::Range { start, end, .. } => {
                self.bind_expr(start);
                self.bind_expr(end);
            }
            ExprKind::TaggedTemplate { tag, template, .. } => {
                self.bind_expr(tag);
                self.bind_expr(template);
            }

            ExprKind::Identifier { .. }
            | ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::RegexLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Super
            | ExprKind::This => {}
        }
    }

    pub(super) fn bind_inline_function(
        &mut self,
        type_params: &[varn_core::ast::TypeParam],
        params: &[varn_core::ast::Param],
        _return_type: Option<&varn_core::ast::TypeNode>,
        body: &varn_core::ast::Stmt,
        range: &varn_core::SourceRange,
    ) {
        use crate::scope::ScopeKind;

        let line = range.start.line;

        let child = self.scopes.child(ScopeKind::Function, self.current);
        let saved = self.current;
        self.current = child;

        self.bind_type_params(type_params, line);
        self.bind_function_params(params, line);

        self.bind_stmt(body);
        self.current = saved;
    }

    fn bind_inline_function_expr(
        &mut self,
        params: &[Param],
        body: &Expr,
        range: &varn_core::SourceRange,
    ) {
        use crate::scope::ScopeKind;

        let child = self.scopes.child(ScopeKind::Function, self.current);
        let saved = self.current;
        self.current = child;
        self.bind_function_params(params, range.start.line);
        self.bind_expr(body);
        self.current = saved;
    }

    fn bind_function_params(&mut self, params: &[Param], line: u32) {
        use super::type_inference::infer_expr_type;
        use super::type_resolution::resolve_type_node;
        use crate::symbol::SymbolKind;
        use crate::types::Type;

        for p in params {
            let ty = p
                .type_ann
                .as_ref()
                .or(match &p.pattern {
                    varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                    _ => None,
                })
                .map(|m| resolve_type_node(m, Some(self)))
                .or_else(|| {
                    p.default
                        .as_ref()
                        .map(|e| infer_expr_type(e, Some(self)))
                        .filter(|t| !t.is_dynamic())
                })
                .unwrap_or(Type::Dynamic);

            self.bind_pattern(&p.pattern, SymbolKind::Parameter, line, None, Some(ty));

            if let Some(default_value) = &p.default {
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
            let mut sym = Symbol::new(SymbolKind::TypeParameter, Rc::from(tp.name.as_str()), line)
                .with_type(Type::named(Rc::from(tp.name.as_str())));
            sym.col = tp.range.start.column;
            sym.offset = tp.range.start.offset;
            self.define(tp.name.clone(), sym);
        }
    }

    fn bind_args(&mut self, args: &[Arg]) {
        for arg in args {
            match arg {
                Arg::Positional(expr) | Arg::Spread(expr) => self.bind_expr(expr),
                Arg::Named { value, .. } => self.bind_expr(value),
            }
        }
    }
}

fn bind_match_pattern_vars(b: &mut super::Binder, pattern: &MatchPattern) {
    use crate::symbol::SymbolKind;
    match pattern {
        MatchPattern::Wildcard | MatchPattern::Type { .. } => {}
        MatchPattern::Literal(expr) => b.bind_expr(expr),
        MatchPattern::EnumVariant {
            variant_name,
            bindings,
            ..
        } => {
            let field_types = b.sum_variant_fields.get(variant_name).cloned();
            for (i, binding) in bindings.iter().enumerate() {
                if binding.name.as_ref() != "_" {
                    let ty = field_types
                        .as_ref()
                        .and_then(|fields| fields.get(i))
                        .map(|(_, t)| t.clone())
                        .unwrap_or(Type::Dynamic);
                    let mut sym = Symbol::new(
                        SymbolKind::Let,
                        binding.name.clone(),
                        binding.range.start.line,
                    )
                    .with_type(ty);
                    sym.col = binding.range.start.column;
                    sym.offset = binding.range.start.offset;
                    b.define(binding.name.to_string(), sym);
                }
            }
        }

        MatchPattern::Identifier(name) => {
            if name.as_ref() != "_" {
                let sym = Symbol::new(SymbolKind::Let, name.clone(), 0).with_type(Type::Dynamic);
                b.define(name.to_string(), sym);
            }
        }
        MatchPattern::Record { fields, .. } => {
            let variant_name = fields.first().and_then(|(key, sub)| {
                if key.as_ref() == "__variant__" {
                    if let Some(MatchPattern::Identifier(n)) = sub {
                        return Some(n.clone());
                    }
                }
                None
            });

            if let Some(vname) = variant_name {
                let field_types: Vec<(Rc<str>, Type)> = b
                    .sum_variant_fields
                    .get(&vname)
                    .cloned()
                    .unwrap_or_default();

                for (field_key, sub_pat) in fields.iter().skip(1) {
                    let binding_name = match sub_pat {
                        Some(MatchPattern::Identifier(n)) => n.clone(),
                        _ => field_key.clone(),
                    };
                    if binding_name.as_ref() == "_" {
                        continue;
                    }

                    let ty = field_types
                        .iter()
                        .find(|(fname, _)| fname.as_ref() == field_key.as_ref())
                        .map(|(_, t)| t.clone())
                        .unwrap_or(Type::Dynamic);
                    let sym = Symbol::new(SymbolKind::Let, binding_name.clone(), 0).with_type(ty);
                    b.define(binding_name.to_string(), sym);

                    if let Some(sub) = sub_pat {
                        if !matches!(sub, MatchPattern::Identifier(_)) {
                            bind_match_pattern_vars(b, sub);
                        }
                    }
                }
            } else {
                for (field_name, sub_pat) in fields {
                    let binding_name = match sub_pat {
                        Some(MatchPattern::Identifier(n)) => n.clone(),
                        _ => field_name.clone(),
                    };
                    if binding_name.as_ref() != "_" {
                        let sym = Symbol::new(SymbolKind::Let, binding_name.clone(), 0)
                            .with_type(Type::Dynamic);
                        b.define(binding_name.to_string(), sym);
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
