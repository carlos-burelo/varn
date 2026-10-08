use varn_core::ast::{
    ArrayEl, ArrowBody, Decl, ExprId, ExprKind, MatchBody, ObjectProp, TemplatePart,
};
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::symbol::{Symbol, SymbolId};

fn type_only(kind: varn_sem::symbol::SymbolKind) -> bool {
    use varn_sem::symbol::SymbolKind as K;
    matches!(kind, K::Interface | K::TypeAlias)
}

impl<'r> super::Binder<'r> {
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
            Decl::SumType(t) => self.bind_sum_type(t),
            Decl::Extension(e) => self.bind_extension(e),
            Decl::Import(i) => self.bind_import(i),
            Decl::Export(e) => self.bind_export(e),
        }
    }

    pub(super) fn define(&mut self, name: varn_core::Atom, sym: Symbol) -> SymbolId {
        let scope = self.scopes.get(self.current);
        if let Some(existing_id) = scope.lookup(name) {
            let existing_sym = self.arena.get(existing_id);
            let existing_from_extern = existing_sym.origin_module.is_some()
                || (existing_sym.line == 0 && existing_sym.full_range.start.line == 0);
            let new_is_import = sym.origin_module.is_some();
            if existing_sym.kind == varn_sem::symbol::SymbolKind::EnumMember
                || sym.kind == varn_sem::symbol::SymbolKind::EnumMember
            {
                let id = self.arena.push(sym);
                self.scopes.get_mut(self.current).define(name, id);
                return id;
            }

            if existing_from_extern && (new_is_import || sym.origin_module.is_none()) {
                let id = self.arena.push(sym);
                self.scopes.get_mut(self.current).define(name, id);
                return id;
            }

            if type_only(existing_sym.kind) != type_only(sym.kind) {
                let id = self.arena.push(sym);
                self.scopes.get_mut(self.current).define(name, id);
                return id;
            }
            let existing_origin = existing_sym
                .origin_module
                .as_ref()
                .map(|m| self.interner.resolve(*m).to_owned())
                .unwrap_or_else(|| self.source_file.to_string());
            let msg = format!(
                "duplicate declaration of '{}' (already declared as {} in {})",
                self.interner.resolve(name),
                existing_sym.kind.label().trim(),
                existing_origin
            );

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
            self.emit(diag);
        }

        let id = self.arena.push(sym);
        self.scopes.get_mut(self.current).define(name, id);
        id
    }

    pub(super) fn bind_expr(&mut self, id: ExprId) {
        let arena = self.ast_arena;
        match &arena.expr(id).kind {
            ExprKind::Missing => {}
            ExprKind::Arrow {
                params,
                return_type,
                body,
                ..
            } => {
                let range = arena.expr(id).range;
                match body.as_ref() {
                    ArrowBody::Block(stmt) => {
                        self.bind_inline_function(&[], params, return_type.as_ref(), *stmt, &range);
                    }
                    ArrowBody::Expr(e) => {
                        self.bind_inline_function_expr(params, *e, &range);
                    }
                }
            }
            ExprKind::Function {
                params,
                return_type,
                body,
                ..
            } => {
                let range = arena.expr(id).range;
                self.bind_inline_function(&[], params, return_type.as_ref(), *body, &range);
            }
            ExprKind::As { expression, .. }
            | ExprKind::Is { expression, .. }
            | ExprKind::Satisfies { expression, .. } => self.bind_expr(*expression),
            ExprKind::MetaAccess { target, .. } => self.bind_expr(*target),
            ExprKind::Await { argument } | ExprKind::Spawn { argument } => {
                self.bind_expr(*argument)
            }
            ExprKind::Try { expression } => self.bind_expr(*expression),
            ExprKind::Yield { argument, .. } => {
                if let Some(arg) = argument {
                    self.bind_expr(*arg);
                }
            }
            ExprKind::Unary { operand, .. } => self.bind_expr(*operand),
            ExprKind::Binary { left, right, .. } | ExprKind::Logical { left, right, .. } => {
                let (left, right) = (*left, *right);
                self.bind_expr(left);
                self.bind_expr(right);
            }
            ExprKind::Assign { op, target, value } => {
                let (op, target, value) = (*op, *target, *value);
                if !self.bind_array_index_write(op, target, value) {
                    self.bind_expr(target);
                    self.bind_expr(value);
                }
            }
            ExprKind::Call { callee, args, .. } => {
                let callee = *callee;
                if !self.bind_array_push_call(callee, args) {
                    self.bind_expr(callee);
                    self.bind_args(args);
                }
            }
            ExprKind::New { callee, args, .. } => {
                self.bind_expr(*callee);
                self.bind_args(args);
            }
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                let (test, consequent, alternate) = (*test, *consequent, *alternate);
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
                let (object, property, computed) = (*object, *property, *computed);
                if !self.bind_array_whitelisted_member(object, property, computed) {
                    self.bind_expr(object);
                    if computed {
                        self.bind_expr(property);
                    }
                }
            }
            ExprKind::Paren { expression } => self.bind_expr(*expression),
            ExprKind::NonNull { expression } => self.bind_expr(*expression),
            ExprKind::Array { elements } => {
                for el in elements {
                    match el {
                        ArrayEl::Expr(e) => self.bind_expr(*e),
                        ArrayEl::Spread(e) => self.bind_expr(*e),
                        ArrayEl::Hole => {}
                    }
                }
            }
            ExprKind::Tuple { elements } => {
                for &e in elements {
                    self.bind_expr(e);
                }
            }
            ExprKind::Object { properties } | ExprKind::Record { properties } => {
                for prop in properties {
                    match prop {
                        ObjectProp::Property { value, .. } => self.bind_expr(*value),
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
                                *body,
                                range,
                            );
                        }
                        ObjectProp::Getter { body, .. } => {
                            self.escape_all_open_array_candidates();
                            self.bind_stmt(*body);
                        }
                        ObjectProp::Setter { body, .. } => {
                            self.escape_all_open_array_candidates();
                            self.bind_stmt(*body);
                        }
                        ObjectProp::Spread { argument, .. } => self.bind_expr(*argument),
                    }
                }
            }
            ExprKind::With { object, properties } => {
                self.bind_expr(*object);
                for prop in properties {
                    match prop {
                        ObjectProp::Property { value, .. } => self.bind_expr(*value),
                        ObjectProp::Spread { argument, .. } => self.bind_expr(*argument),
                        ObjectProp::Method { .. }
                        | ObjectProp::Getter { .. }
                        | ObjectProp::Setter { .. } => {}
                    }
                }
            }
            ExprKind::Template { parts } => {
                for p in parts {
                    if let TemplatePart::Interpolation(e) = p {
                        self.bind_expr(*e);
                    }
                }
            }
            ExprKind::Sequence { expressions } => {
                for &e in expressions {
                    self.bind_expr(e);
                }
            }
            ExprKind::ClassExpr { declaration } => {
                self.bind_class(declaration);
            }
            ExprKind::Match { subject, cases } => {
                let subject = *subject;
                self.bind_expr(subject);
                let mut arm_scopes = Vec::with_capacity(cases.len());
                for case in cases {
                    use varn_sem::scope::ScopeKind;

                    let child = self.scopes.child(ScopeKind::Block, self.current);
                    arm_scopes.push(child);
                    let saved = self.current;
                    self.current = child;

                    super::decls_fn::bind_match_pattern_vars(self, &case.pattern);

                    if let Some(g) = case.guard {
                        self.bind_expr(g);
                    }
                    match &case.body {
                        MatchBody::Expr(e) => self.bind_expr(*e),
                        MatchBody::Block(stmt) => self.bind_stmt(*stmt),
                    }
                    self.finalize_array_watch(child);
                    self.current = saved;
                }
                self.match_arm_scopes.insert(subject.index(), arm_scopes);
            }
            ExprKind::Update { operand, .. } => self.bind_expr(*operand),
            ExprKind::Spread { argument } => self.bind_expr(*argument),
            ExprKind::Pipeline { left, right } => {
                let (left, right) = (*left, *right);
                self.bind_expr(left);
                self.bind_expr(right);
            }
            ExprKind::Range { start, end, .. } => {
                let (start, end) = (*start, *end);
                self.bind_expr(start);
                self.bind_expr(end);
            }
            ExprKind::TaggedTemplate { tag, template, .. } => {
                let (tag, template) = (*tag, *template);
                self.bind_expr(tag);
                self.bind_expr(template);
            }

            ExprKind::Identifier { name } => {
                self.escape_array_candidate(*name);
                let range = arena.expr(id).range;
                self.check_local_class_capture(*name, range);
            }
            ExprKind::IntLiteral { .. }
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
}
