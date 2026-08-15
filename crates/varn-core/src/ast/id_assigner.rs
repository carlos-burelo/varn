use super::decl::{ClassMember, Decl, ExportDecl, ExportDefaultDecl, ExtensionMember};
use super::expr::{Arg, ArrayEl, ArrowBody, AstId, ObjectProp, PropKey, TemplatePart};
use super::pattern::MatchPattern;
use super::stmt::{ForInit, Stmt};
use super::types::{Decorator, TypeNode};
use super::{AstMetadata, ClassDecl, Expr, ExprKind, MatchBody, Program, StmtKind};
use crate::kinds::TypeKind;

struct IdAssigner<'a> {
    next: AstId,
    metadata: &'a mut AstMetadata,
}

impl<'a> IdAssigner<'a> {
    fn new(metadata: &'a mut AstMetadata) -> Self {
        Self { next: 1, metadata }
    }

    fn next_id(&mut self) -> AstId {
        let id = self.next;
        self.next += 1;
        id
    }

    fn assign_program(&mut self, program: &mut Program) {
        for s in &mut program.body {
            self.assign_stmt(s);
        }
    }

    fn assign_stmt(&mut self, stmt: &mut Stmt) {
        stmt.id = self.next_id();
        self.metadata.add(stmt.id, stmt.range);
        match &mut stmt.kind {
            StmtKind::Block { stmts, .. } => {
                for s in stmts {
                    self.assign_stmt(s);
                }
            }
            StmtKind::Empty => {}
            StmtKind::Expr { expression, .. } => {
                self.assign_expr(expression);
            }
            StmtKind::Decl(decl) => {
                self.assign_decl_ids(decl);
            }
            StmtKind::If {
                test,
                consequent,
                alternate,
                ..
            } => {
                self.assign_expr(test);
                self.assign_stmt(consequent);
                if let Some(alt) = alternate {
                    self.assign_stmt(alt);
                }
            }
            StmtKind::While { test, body, .. } => {
                self.assign_expr(test);
                self.assign_stmt(body);
            }
            StmtKind::DoWhile { body, test, .. } => {
                self.assign_stmt(body);
                self.assign_expr(test);
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
                ..
            } => {
                if let Some(i) = init {
                    self.assign_for_init(i);
                }
                if let Some(t) = test {
                    self.assign_expr(t);
                }
                if let Some(u) = update {
                    self.assign_expr(u);
                }
                self.assign_stmt(body);
            }
            StmtKind::ForIn { right, body, .. } => {
                self.assign_expr(right);
                self.assign_stmt(body);
            }
            StmtKind::ForOf { right, body, .. } => {
                self.assign_expr(right);
                self.assign_stmt(body);
            }
            StmtKind::Switch {
                discriminant,
                cases,
                ..
            } => {
                self.assign_expr(discriminant);
                for case in cases {
                    if let Some(t) = &mut case.test {
                        self.assign_expr(t);
                    }
                    for s in &mut case.body {
                        self.assign_stmt(s);
                    }
                }
            }
            StmtKind::Return { argument, .. } => {
                if let Some(a) = argument {
                    self.assign_expr(a);
                }
            }
            StmtKind::Break { .. } => {}
            StmtKind::Continue { .. } => {}
            StmtKind::Throw { argument, .. } => {
                self.assign_expr(argument);
            }
            StmtKind::Try {
                block,
                catch,
                finally,
                ..
            } => {
                self.assign_stmt(block);
                if let Some(c) = catch {
                    self.assign_stmt(&mut c.body);
                }
                if let Some(f) = finally {
                    self.assign_stmt(f);
                }
            }
            StmtKind::Using { declarations, .. } => {
                for declarator in declarations {
                    if let Some(ann) = &mut declarator.type_ann {
                        self.assign_type_node(ann);
                    }
                    if let Some(init) = &mut declarator.init {
                        self.assign_expr(init);
                    }
                }
            }
            StmtKind::Labeled { body, .. } => {
                self.assign_stmt(body);
            }
            StmtKind::Debugger => {}
        }
    }

    fn assign_for_init(&mut self, fi: &mut ForInit) {
        match fi {
            ForInit::Var { .. } => {}
            ForInit::Expr(e) => self.assign_expr(e),
        }
    }

    fn assign_expr(&mut self, expr: &mut Expr) {
        expr.id = self.next_id();
        self.metadata.add(expr.id, expr.range);
        match &mut expr.kind {
            ExprKind::IntLiteral { .. } => {}
            ExprKind::FloatLiteral { .. } => {}
            ExprKind::BigIntLiteral { .. } => {}
            ExprKind::DecimalLiteral { .. } => {}
            ExprKind::StrLiteral { .. } => {}
            ExprKind::CharLiteral { .. } => {}
            ExprKind::BoolLiteral { .. } => {}
            ExprKind::NullLiteral => {}
            ExprKind::RegexLiteral { .. } => {}
            ExprKind::Template { parts, .. } => {
                for part in parts {
                    if let TemplatePart::Interpolation(ref mut e) = part {
                        self.assign_expr(e);
                    }
                }
            }
            ExprKind::TaggedTemplate { tag, template, .. } => {
                self.assign_expr(tag);
                self.assign_expr(template);
            }
            ExprKind::Identifier { .. } => {}
            ExprKind::This => {}
            ExprKind::Super => {}
            ExprKind::Array { elements, .. } => {
                for el in elements {
                    if let ArrayEl::Expr(ref mut e) | ArrayEl::Spread(ref mut e) = el {
                        self.assign_expr(e);
                    }
                }
            }
            ExprKind::Object { properties, .. } => {
                for prop in properties {
                    self.assign_object_prop(prop);
                }
            }
            ExprKind::Tuple { elements } => {
                for el in elements {
                    self.assign_expr(el);
                }
            }
            ExprKind::Record { properties } => {
                for prop in properties {
                    self.assign_object_prop(prop);
                }
            }
            ExprKind::Unary { operand, .. } => {
                self.assign_expr(operand);
            }
            ExprKind::Update { operand, .. } => {
                self.assign_expr(operand);
            }
            ExprKind::Binary { left, right, .. } => {
                self.assign_expr(left);
                self.assign_expr(right);
            }
            ExprKind::Logical { left, right, .. } => {
                self.assign_expr(left);
                self.assign_expr(right);
            }
            ExprKind::Assign { target, value, .. } => {
                self.assign_expr(target);
                self.assign_expr(value);
            }
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
                ..
            } => {
                self.assign_expr(test);
                self.assign_expr(consequent);
                self.assign_expr(alternate);
            }
            ExprKind::Member {
                object, property, ..
            } => {
                self.assign_expr(object);
                self.assign_expr(property);
            }
            ExprKind::Call { callee, args, .. } => {
                self.assign_expr(callee);
                for arg in args {
                    match arg {
                        Arg::Positional(ref mut e) | Arg::Spread(ref mut e) => self.assign_expr(e),
                        Arg::Named { ref mut value, .. } => self.assign_expr(value),
                    }
                }
            }
            ExprKind::New { callee, args, .. } => {
                self.assign_expr(callee);
                for arg in args {
                    match arg {
                        Arg::Positional(ref mut e) | Arg::Spread(ref mut e) => self.assign_expr(e),
                        Arg::Named { ref mut value, .. } => self.assign_expr(value),
                    }
                }
            }
            ExprKind::Function {
                body,
                params,
                return_type,
                ..
            } => {
                if let Some(rt) = return_type {
                    self.assign_type_node(rt);
                }
                for p in params {
                    if let Some(ref mut def) = p.default {
                        self.assign_expr(def);
                    }
                }
                self.assign_stmt(body);
            }
            ExprKind::Arrow {
                params,
                body,
                return_type,
                ..
            } => {
                if let Some(rt) = return_type {
                    self.assign_type_node(rt);
                }
                for p in params {
                    if let Some(ref mut def) = p.default {
                        self.assign_expr(def);
                    }
                }
                match &mut **body {
                    ArrowBody::Block(ref mut s) => self.assign_stmt(s),
                    ArrowBody::Expr(ref mut e) => self.assign_expr(e),
                }
            }
            ExprKind::Sequence { expressions, .. } => {
                for e in expressions {
                    self.assign_expr(e);
                }
            }
            ExprKind::Paren { expression, .. } => {
                self.assign_expr(expression);
            }
            ExprKind::Await { argument, .. } => {
                self.assign_expr(argument);
            }
            ExprKind::Spawn { argument, .. } => {
                self.assign_expr(argument);
            }
            ExprKind::Yield { argument, .. } => {
                if let Some(a) = argument {
                    self.assign_expr(a);
                }
            }
            ExprKind::Spread { argument, .. } => {
                self.assign_expr(argument);
            }
            ExprKind::Pipeline { left, right, .. } => {
                self.assign_expr(left);
                self.assign_expr(right);
            }
            ExprKind::Range { start, end, .. } => {
                self.assign_expr(start);
                self.assign_expr(end);
            }
            ExprKind::NonNull { expression, .. } => {
                self.assign_expr(expression);
            }
            ExprKind::Try { expression, .. } => {
                self.assign_expr(expression);
            }
            ExprKind::As { expression, .. } => {
                self.assign_expr(expression);
            }
            ExprKind::Satisfies {
                expression,
                type_ann,
                ..
            } => {
                self.assign_expr(expression);
                self.assign_type_node(type_ann);
            }
            ExprKind::ClassExpr { declaration, .. } => {
                self.assign_class_decl(declaration);
            }
            ExprKind::Match { subject, cases, .. } => {
                self.assign_expr(subject);
                for case in cases {
                    self.assign_match_pattern(&mut case.pattern);
                    if let Some(ref mut g) = case.guard {
                        self.assign_expr(g);
                    }
                    match &mut case.body {
                        MatchBody::Block(ref mut s) => self.assign_stmt(s),
                        MatchBody::Expr(ref mut e) => self.assign_expr(e),
                    }
                }
            }
            ExprKind::Is {
                expression,
                type_ann,
                ..
            } => {
                self.assign_expr(expression);
                self.assign_type_node(type_ann);
            }
        }
    }

    fn assign_object_prop(&mut self, prop: &mut ObjectProp) {
        match prop {
            ObjectProp::Property { key, value, .. } => {
                if let PropKey::Computed(e) = key {
                    self.assign_expr(e);
                }
                self.assign_expr(value);
            }
            ObjectProp::Method {
                key, params, body, ..
            } => {
                if let PropKey::Computed(e) = key {
                    self.assign_expr(e);
                }
                for p in params {
                    if let Some(def) = &mut p.default {
                        self.assign_expr(def);
                    }
                }
                self.assign_stmt(body);
            }
            ObjectProp::Getter { key, body, .. } => {
                if let PropKey::Computed(e) = key {
                    self.assign_expr(e);
                }
                self.assign_stmt(body);
            }
            ObjectProp::Setter {
                key, param, body, ..
            } => {
                if let PropKey::Computed(e) = key {
                    self.assign_expr(e);
                }
                if let Some(def) = &mut param.default {
                    self.assign_expr(def);
                }
                self.assign_stmt(body);
            }
            ObjectProp::Spread { argument, .. } => {
                self.assign_expr(argument);
            }
        }
    }

    fn assign_match_pattern(&mut self, pat: &mut MatchPattern) {
        match pat {
            MatchPattern::Wildcard => {}
            MatchPattern::Literal(e) => self.assign_expr(e),
            MatchPattern::Identifier(_) => {}
            MatchPattern::EnumVariant { .. } => {}
            MatchPattern::Record { fields, .. } => {
                for (_, sub) in fields {
                    if let Some(s) = sub {
                        self.assign_match_pattern(s);
                    }
                }
            }
            MatchPattern::Sequence(patterns) => {
                for p in patterns {
                    self.assign_match_pattern(p);
                }
            }
            MatchPattern::Type { .. } => {}
        }
    }

    fn assign_decl_ids(&mut self, decl: &mut Decl) {
        match decl {
            Decl::Variable(v) => {
                v.ast_id = self.next_id();
                self.metadata.add(v.ast_id, v.range);
                for d in &mut v.declarators {
                    self.metadata.add(v.ast_id, d.range);
                    if let Some(ann) = &mut d.type_ann {
                        self.assign_type_node(ann);
                    }
                    if let Some(init) = &mut d.init {
                        self.assign_expr(init);
                    }
                }
            }
            Decl::Function(f) => {
                f.ast_id = self.next_id();
                self.metadata.add(f.ast_id, f.range);
                self.assign_decorators(&mut f.decorators);
                if let Some(rt) = &mut f.return_type {
                    self.assign_type_node(rt);
                }
                for p in &mut f.params {
                    if let Some(def) = &mut p.default {
                        self.assign_expr(def);
                    }
                }
                self.assign_stmt(&mut f.body);
            }
            Decl::Class(c) => {
                c.ast_id = self.next_id();
                self.metadata.add(c.ast_id, c.range);
                self.assign_class_decl(c);
            }
            Decl::Interface(i) => {
                i.ast_id = self.next_id();
                for ext in &mut i.extends {
                    self.assign_type_node(ext);
                }
                for m in &mut i.body {
                    match m {
                        super::decl::InterfaceMember::Property { type_ann, .. } => {
                            self.assign_type_node(type_ann);
                        }
                        super::decl::InterfaceMember::Method {
                            params,
                            return_type,
                            ..
                        } => {
                            for p in params {
                                if let Some(ann) = &mut p.type_ann {
                                    self.assign_type_node(ann);
                                }
                            }
                            if let Some(rt) = return_type {
                                self.assign_type_node(rt);
                            }
                        }
                        super::decl::InterfaceMember::Index {
                            param, return_type, ..
                        } => {
                            if let Some(ann) = &mut param.type_ann {
                                self.assign_type_node(ann);
                            }
                            self.assign_type_node(return_type);
                        }
                        super::decl::InterfaceMember::Callable {
                            params,
                            return_type,
                            ..
                        } => {
                            for p in params {
                                if let Some(ann) = &mut p.type_ann {
                                    self.assign_type_node(ann);
                                }
                            }
                            self.assign_type_node(return_type);
                        }
                    }
                }
            }
            Decl::TypeAlias(a) => {
                a.ast_id = self.next_id();
                self.assign_type_node(&mut a.alias);
            }
            Decl::Enum(e) => {
                e.ast_id = self.next_id();
                for tp in &mut e.type_params {
                    if let Some(c) = &mut tp.constraint {
                        self.assign_type_node(c);
                    }
                    if let Some(d) = &mut tp.default {
                        self.assign_type_node(d);
                    }
                }
                for imp in &mut e.implements {
                    self.assign_type_node(imp);
                }
                for m in &mut e.members {
                    if let Some(init) = &mut m.init {
                        self.assign_expr(init);
                    }
                    for field in &mut m.payload_fields {
                        self.assign_type_node(&mut field.ty);
                    }
                }
                self.assign_class_members(&mut e.body);
            }
            Decl::Namespace(n) => {
                n.ast_id = self.next_id();
                for d in &mut n.body {
                    self.assign_decl_ids(d);
                }
            }
            Decl::Import(i) => {
                i.ast_id = self.next_id();
            }
            Decl::Export(e) => match e {
                ExportDecl::Named { ast_id, .. } => {
                    *ast_id = self.next_id();
                }
                ExportDecl::Default {
                    ast_id,
                    declaration,
                    ..
                } => {
                    *ast_id = self.next_id();
                    match &mut **declaration {
                        ExportDefaultDecl::Function(f) => {
                            f.ast_id = self.next_id();
                            self.assign_decorators(&mut f.decorators);
                            if let Some(rt) = &mut f.return_type {
                                self.assign_type_node(rt);
                            }
                            for p in &mut f.params {
                                if let Some(def) = &mut p.default {
                                    self.assign_expr(def);
                                }
                            }
                            self.assign_stmt(&mut f.body);
                        }
                        ExportDefaultDecl::Class(c) => {
                            c.ast_id = self.next_id();
                            self.assign_class_decl(c);
                        }
                        ExportDefaultDecl::Expr(expr) => {
                            self.assign_expr(expr);
                        }
                    }
                }
                ExportDecl::Decl {
                    ast_id,
                    declaration,
                    ..
                } => {
                    *ast_id = self.next_id();
                    self.assign_decl_ids(declaration);
                }
                ExportDecl::All { ast_id, .. } => {
                    *ast_id = self.next_id();
                }
            },
            Decl::Extension(ext) => {
                ext.ast_id = self.next_id();
                self.assign_type_node(&mut ext.target);
                for member in &mut ext.members {
                    match member {
                        ExtensionMember::Method(f) => {
                            if let Some(rt) = &mut f.return_type {
                                self.assign_type_node(rt);
                            }
                            for p in &mut f.params {
                                if let Some(def) = &mut p.default {
                                    self.assign_expr(def);
                                }
                            }
                            self.assign_stmt(&mut f.body);
                        }
                        ExtensionMember::Getter {
                            body, return_type, ..
                        } => {
                            if let Some(rt) = return_type {
                                self.assign_type_node(rt);
                            }
                            self.assign_stmt(body);
                        }
                        ExtensionMember::Setter { body, param, .. } => {
                            if let Some(ann) = &mut param.type_ann {
                                self.assign_type_node(ann);
                            }
                            if let Some(def) = &mut param.default {
                                self.assign_expr(def);
                            }
                            self.assign_stmt(body);
                        }
                    }
                }
            }
            Decl::Struct(s) => {
                s.ast_id = self.next_id();
                for f in &mut s.fields {
                    self.assign_type_node(&mut f.type_ann);
                    if let Some(init) = &mut f.default {
                        self.assign_expr(init);
                    }
                }
            }
            Decl::SumType(s) => {
                s.ast_id = self.next_id();
                for v in &mut s.variants {
                    for f in &mut v.fields {
                        self.assign_type_node(&mut f.ty);
                    }
                }
            }
        }
    }

    fn assign_decorators(&mut self, decorators: &mut [Decorator]) {
        for d in decorators {
            self.assign_expr(&mut d.expression);
        }
    }

    fn assign_class_decl(&mut self, c: &mut ClassDecl) {
        self.assign_decorators(&mut c.decorators);
        self.assign_class_members(&mut c.body);
        if let Some(sc) = &mut c.super_class {
            self.assign_expr(sc);
        }
        for e in &mut c.super_type_args {
            self.assign_type_node(e);
        }
        for imp in &mut c.implements {
            self.assign_type_node(imp);
        }
    }

    fn assign_class_members(&mut self, members: &mut [ClassMember]) {
        for member in members {
            match member {
                ClassMember::Constructor {
                    ref mut body,
                    ref mut params,
                    ..
                } => {
                    for p in params {
                        if let Some(ann) = &mut p.type_ann {
                            self.assign_type_node(ann);
                        }
                        if let Some(def) = &mut p.default {
                            self.assign_expr(def);
                        }
                    }
                    self.assign_stmt(body);
                }
                ClassMember::Destructor { ref mut body, .. } => {
                    self.assign_stmt(body);
                }
                ClassMember::Method {
                    ref mut body,
                    ref mut params,
                    ref mut decorators,
                    ref mut return_type,
                    ..
                } => {
                    self.assign_decorators(decorators);
                    if let Some(rt) = return_type {
                        self.assign_type_node(rt);
                    }
                    for p in params {
                        if let Some(ann) = &mut p.type_ann {
                            self.assign_type_node(ann);
                        }
                        if let Some(def) = &mut p.default {
                            self.assign_expr(def);
                        }
                    }
                    if let Some(b) = body {
                        self.assign_stmt(b);
                    }
                }
                ClassMember::Property {
                    ref mut init,
                    ref mut type_ann,
                    ..
                } => {
                    if let Some(ann) = type_ann {
                        self.assign_type_node(ann);
                    }
                    if let Some(e) = init {
                        self.assign_expr(e);
                    }
                }
                ClassMember::Getter {
                    ref mut body,
                    ref mut return_type,
                    ..
                } => {
                    if let Some(rt) = return_type {
                        self.assign_type_node(rt);
                    }
                    if let Some(b) = body {
                        self.assign_stmt(b);
                    }
                }
                ClassMember::Setter {
                    ref mut body,
                    ref mut param,
                    ..
                } => {
                    if let Some(ann) = &mut param.type_ann {
                        self.assign_type_node(ann);
                    }
                    if let Some(def) = &mut param.default {
                        self.assign_expr(def);
                    }
                    if let Some(b) = body {
                        self.assign_stmt(b);
                    }
                }
                ClassMember::StaticBlock { ref mut body, .. } => {
                    self.assign_stmt(body);
                }
            }
        }
    }

    fn assign_type_node(&mut self, node: &mut TypeNode) {
        node.id = self.next_id();
        self.metadata.add(node.id, node.range);
        match &mut node.kind {
            TypeKind::Array(inner) => self.assign_type_node(inner),
            TypeKind::Union(members) => {
                for m in members {
                    self.assign_type_node(m);
                }
            }
            TypeKind::Generic(_, args, _) => {
                for a in args {
                    self.assign_type_node(a);
                }
            }
            TypeKind::Fn((params, ret)) => {
                for p in params {
                    if let Some(c) = &mut p.constraint {
                        self.assign_type_node(c);
                    }
                    if let Some(d) = &mut p.default {
                        self.assign_type_node(d);
                    }
                }
                self.assign_type_node(ret);
            }
            TypeKind::Object(members) => {
                for m in members {
                    match m {
                        super::decl::InterfaceMember::Property { type_ann, .. } => {
                            self.assign_type_node(type_ann);
                        }
                        super::decl::InterfaceMember::Method {
                            params,
                            return_type,
                            ..
                        } => {
                            for p in params {
                                if let Some(ann) = &mut p.type_ann {
                                    self.assign_type_node(ann);
                                }
                            }
                            if let Some(rt) = return_type {
                                self.assign_type_node(rt);
                            }
                        }
                        super::decl::InterfaceMember::Index {
                            param, return_type, ..
                        } => {
                            if let Some(ann) = &mut param.type_ann {
                                self.assign_type_node(ann);
                            }
                            self.assign_type_node(return_type);
                        }
                        super::decl::InterfaceMember::Callable {
                            params,
                            return_type,
                            ..
                        } => {
                            for p in params {
                                if let Some(ann) = &mut p.type_ann {
                                    self.assign_type_node(ann);
                                }
                            }
                            self.assign_type_node(return_type);
                        }
                    }
                }
            }
            TypeKind::Typeof(e) => {
                self.assign_expr(e);
            }
            _ => {}
        }
    }
}

pub fn assign_ast_ids(program: &mut Program) {
    let mut metadata = std::mem::take(&mut program.metadata);
    {
        let mut assigner = IdAssigner::new(&mut metadata);
        assigner.assign_program(program);
    }
    program.metadata = metadata;
}
