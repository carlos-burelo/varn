//! The document's syntax tree as JSON, for the editor's tree view.
//!
//! Names are resolved through the document's interner; patterns and types
//! are shown as the source that wrote them.

use serde_json::Value;
use varn_core::ast::{
    Arg, ClassMember, Decl, ExportDecl, ExprId, ExprKind, ForInit, InterfaceMember, MatchBody,
    StmtId, StmtKind, VarDeclarator, VarKind,
};

use crate::document::DocumentState;

pub fn dump_ast_json(state: &DocumentState) -> Result<Value, String> {
    let program = state
        .ast
        .as_ref()
        .ok_or_else(|| "No AST available".to_string())?;
    let tree = AstJson { state };
    Ok(Value::Array(
        program.body.iter().map(|&s| tree.stmt(s)).collect(),
    ))
}

fn node(label: String, kind: &str, line: u32, children: Vec<Value>) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("label".to_string(), Value::String(label));
    map.insert("kind".to_string(), Value::String(kind.to_string()));
    map.insert(
        "line".to_string(),
        Value::Number(serde_json::Number::from(line.saturating_sub(1))),
    );
    if !children.is_empty() {
        map.insert("children".to_string(), Value::Array(children));
    }
    Value::Object(map)
}

fn leaf(label: String, kind: &str, line: u32) -> Value {
    node(label, kind, line, Vec::new())
}

fn var_kind(kind: VarKind) -> &'static str {
    match kind {
        VarKind::Let => "let",
        VarKind::Const => "const",
    }
}

struct AstJson<'a> {
    state: &'a DocumentState,
}

impl AstJson<'_> {
    fn name(&self, atom: varn_core::Atom) -> &str {
        self.state.name(atom)
    }

    fn text(&self, range: varn_core::SourceRange) -> &str {
        self.state.source_text(range)
    }

    fn stmt(&self, id: StmtId) -> Value {
        let stmt = self.state.ast_arena.stmt(id);
        let line = stmt.range.start.line;
        match &stmt.kind {
            StmtKind::Decl(decl) => self.decl(decl, line),
            StmtKind::Block { stmts } => {
                let children = stmts.iter().map(|&s| self.stmt(s)).collect();
                node("Block".to_string(), "method", line, children)
            }
            StmtKind::Expr { expression } => node(
                "ExprStmt".to_string(),
                "statement",
                line,
                vec![self.expr(*expression)],
            ),
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                let mut children = vec![self.expr(*test), self.stmt(*consequent)];
                children.extend(alternate.map(|alt| self.stmt(alt)));
                node("IfStmt".to_string(), "keyword", line, children)
            }
            StmtKind::While { test, body } => node(
                "WhileStmt".to_string(),
                "keyword",
                line,
                vec![self.expr(*test), self.stmt(*body)],
            ),
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                let mut children = Vec::new();
                match init.as_deref() {
                    Some(ForInit::Var { kind, declarators }) => {
                        children.extend(declarators.iter().map(|d| self.declarator(*kind, d)));
                    }
                    Some(ForInit::Expr(e)) => children.push(self.expr(*e)),
                    None => {}
                }
                children.extend(test.map(|t| self.expr(t)));
                children.extend(update.map(|u| self.expr(u)));
                children.push(self.stmt(*body));
                node("ForStmt".to_string(), "keyword", line, children)
            }
            StmtKind::ForIn {
                left, right, body, ..
            } => node(
                format!("for {} in", self.text(*left.range())),
                "keyword",
                line,
                vec![self.expr(*right), self.stmt(*body)],
            ),
            StmtKind::ForOf {
                left, right, body, ..
            } => node(
                format!("for {} of", self.text(*left.range())),
                "keyword",
                line,
                vec![self.expr(*right), self.stmt(*body)],
            ),
            StmtKind::Return { argument } => {
                let children = argument.iter().map(|&a| self.expr(a)).collect();
                node("ReturnStmt".to_string(), "keyword", line, children)
            }
            StmtKind::Break { .. } => leaf("BreakStmt".to_string(), "keyword", line),
            StmtKind::Continue { .. } => leaf("ContinueStmt".to_string(), "keyword", line),
            StmtKind::Empty => leaf("EmptyStmt".to_string(), "field", line),
            StmtKind::Error => leaf("ErrorStmt".to_string(), "field", line),
            _ => leaf("Stmt".to_string(), "statement", line),
        }
    }

    fn declarator(&self, kind: VarKind, d: &VarDeclarator) -> Value {
        node(
            format!("{} {}", var_kind(kind), self.text(*d.id.range())),
            "variable",
            d.range.start.line,
            d.init.iter().map(|&init| self.expr(init)).collect(),
        )
    }

    fn decl(&self, decl: &Decl, line: u32) -> Value {
        match decl {
            Decl::Function(f) => {
                let mut children: Vec<Value> = f
                    .params
                    .iter()
                    .map(|p| {
                        leaf(
                            format!("param: {}", self.text(p.range)),
                            "parameter",
                            p.range.start.line,
                        )
                    })
                    .collect();
                children.push(self.stmt(f.body));
                node(
                    format!("fn {}", self.name(f.id)),
                    "function",
                    line,
                    children,
                )
            }
            Decl::Class(c) => {
                let class_name = c.id.map_or("Anonymous", |id| self.name(id));
                let children = c
                    .body
                    .iter()
                    .filter_map(|member| match member {
                        ClassMember::Property { key, range, .. } => Some(leaf(
                            format!("prop {}", self.name(*key)),
                            "property",
                            range.start.line,
                        )),
                        ClassMember::Method {
                            key, body, range, ..
                        } => Some(node(
                            format!("method {}", self.name(*key)),
                            "method",
                            range.start.line,
                            body.iter().map(|&b| self.stmt(b)).collect(),
                        )),
                        ClassMember::Constructor { range, body, .. } => Some(node(
                            "constructor".to_string(),
                            "method",
                            range.start.line,
                            vec![self.stmt(*body)],
                        )),
                        _ => None,
                    })
                    .collect();
                node(format!("class {class_name}"), "class", line, children)
            }
            Decl::Interface(i) => {
                let children = i
                    .body
                    .iter()
                    .filter_map(|m| match m {
                        InterfaceMember::Method { key, range, .. } => Some(leaf(
                            format!("method {}", self.name(*key)),
                            "method",
                            range.start.line,
                        )),
                        InterfaceMember::Property { key, range, .. } => Some(leaf(
                            format!("prop {}", self.name(*key)),
                            "property",
                            range.start.line,
                        )),
                        _ => None,
                    })
                    .collect();
                let name = self.name(i.id);
                node(format!("interface {name}"), "interface", line, children)
            }
            Decl::Enum(e) => {
                let children = e
                    .members
                    .iter()
                    .map(|m| {
                        leaf(
                            format!("variant {}", self.name(m.id)),
                            "enum_member",
                            m.range.start.line,
                        )
                    })
                    .collect();
                node(format!("enum {}", self.name(e.id)), "enum", line, children)
            }
            Decl::Struct(s) => leaf(format!("struct {}", self.name(s.id)), "struct", line),
            Decl::TypeAlias(t) => leaf(format!("type {}", self.name(t.id)), "type", line),
            Decl::Import(imp) => leaf(
                format!("import '{}'", self.name(imp.source)),
                "import",
                line,
            ),
            Decl::Export(ExportDecl::Decl { declaration, .. }) => node(
                "export".to_string(),
                "export",
                line,
                vec![self.decl(declaration, line)],
            ),
            Decl::Export(_) => leaf("export".to_string(), "export", line),
            Decl::Variable(v) => {
                let children = v
                    .declarators
                    .iter()
                    .map(|d| self.declarator(v.kind, d))
                    .collect();
                node(
                    format!("{} declaration", var_kind(v.kind)),
                    "variable",
                    line,
                    children,
                )
            }
            Decl::Extension(ext) => leaf(
                format!("extension {}", self.text(ext.target.range)),
                "class",
                line,
            ),
            _ => leaf("Decl".to_string(), "field", line),
        }
    }

    fn expr(&self, id: ExprId) -> Value {
        let expr = self.state.ast_arena.expr(id);
        let line = expr.range.start.line;
        match &expr.kind {
            ExprKind::Identifier { name } => {
                leaf(format!("Ident: {}", self.name(*name)), "variable", line)
            }
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::NullLiteral => leaf(self.text(expr.range).to_owned(), "constant", line),
            ExprKind::Binary { left, op, right } => node(
                format!("BinaryExpr ({op:?})"),
                "operator",
                line,
                vec![self.expr(*left), self.expr(*right)],
            ),
            ExprKind::Unary { op, operand, .. } => node(
                format!("UnaryExpr ({op:?})"),
                "operator",
                line,
                vec![self.expr(*operand)],
            ),
            ExprKind::Call { callee, args, .. } => {
                let mut children = vec![self.expr(*callee)];
                for arg in args {
                    children.push(match arg {
                        Arg::Positional(e) | Arg::Spread(e) => self.expr(*e),
                        Arg::Named { label, value } => node(
                            format!("{label}:"),
                            "parameter",
                            self.state.ast_arena.expr(*value).range.start.line,
                            vec![self.expr(*value)],
                        ),
                    });
                }
                node("CallExpr".to_string(), "keyword", line, children)
            }
            ExprKind::Member {
                object,
                property,
                computed,
                ..
            } => {
                let label = if *computed { "IndexExpr" } else { "MemberExpr" };
                node(
                    label.to_string(),
                    "property",
                    line,
                    vec![self.expr(*object), self.expr(*property)],
                )
            }
            ExprKind::Pipeline { left, right } => node(
                "Pipeline (|>)".to_string(),
                "operator",
                line,
                vec![self.expr(*left), self.expr(*right)],
            ),
            ExprKind::Assign { op, target, value } => node(
                format!("AssignExpr ({op:?})"),
                "operator",
                line,
                vec![self.expr(*target), self.expr(*value)],
            ),
            ExprKind::Match { subject, cases } => {
                let mut children = vec![self.expr(*subject)];
                for case in cases {
                    let body = match &case.body {
                        MatchBody::Expr(e) => self.expr(*e),
                        MatchBody::Block(b) => self.stmt(*b),
                    };
                    children.push(node(
                        "MatchCase".to_string(),
                        "keyword",
                        case.range.start.line,
                        vec![body],
                    ));
                }
                node("MatchExpr".to_string(), "keyword", line, children)
            }
            _ => leaf("Expr".to_string(), "statement", line),
        }
    }
}
