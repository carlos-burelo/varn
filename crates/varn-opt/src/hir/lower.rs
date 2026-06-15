//! AST -> HIR lowering for the imperative core.
//!
//! Handles: top-level function declarations (each becomes a module global),
//! top-level statements, and inside functions — `let`/assignment/`return`/`if`/
//! `while`/`for`, calls, typed binary/unary ops, literals, and identifier
//! resolution (param / local / global). Anything outside this core returns
//! `OptError::Unsupported`, and the whole program falls back to legacy codegen
//! (the supported subset grows stage by stage).

use std::rc::Rc;

use varn_core::ast::operators::{AssignOp, BinaryOp, UnaryOp};
use varn_core::ast::{
    Arg, Decl, Expr, ExprKind, ForInit, FunctionDecl, Param, Pattern, Stmt, StmtKind,
};
use varn_core::{NumericKind, TypeAnnotations};

use crate::hir::*;
use crate::{OptError, OptInput};

type R<T> = Result<T, OptError>;

fn unsupported<T>(what: &'static str) -> R<T> {
    Err(OptError::Unsupported(what))
}

/// Lexical scope chain mapping names to resolved HIR bindings within one
/// function (plus the module-global set, shared across functions).
struct Scope {
    /// One map per nested block; innermost last.
    blocks: Vec<rustc_hash::FxHashMap<Rc<str>, HirBinding>>,
    next_local: u32,
}

impl Scope {
    fn new() -> Self {
        Self {
            blocks: vec![rustc_hash::FxHashMap::default()],
            next_local: 0,
        }
    }
    fn push(&mut self) {
        self.blocks.push(rustc_hash::FxHashMap::default());
    }
    fn pop(&mut self) {
        self.blocks.pop();
    }
    fn define(&mut self, name: Rc<str>, binding: HirBinding) {
        self.blocks.last_mut().unwrap().insert(name, binding);
    }
    fn alloc_local(&mut self, name: Rc<str>) -> LocalId {
        let id = LocalId(self.next_local);
        self.next_local += 1;
        self.define(name, HirBinding::Local(id));
        id
    }
    fn resolve(&self, name: &str) -> Option<HirBinding> {
        for block in self.blocks.iter().rev() {
            if let Some(b) = block.get(name) {
                return Some(b.clone());
            }
        }
        None
    }
}

pub struct Lowerer<'a> {
    ann: &'a TypeAnnotations,
    /// Names declared as module globals (top-level functions and `let`s), so
    /// identifiers that don't resolve locally can be classified as globals.
    globals: rustc_hash::FxHashMap<Rc<str>, ()>,
}

/// Lower a whole program to HIR. Top-level function declarations become module
/// globals; remaining top-level statements form the synthetic module function.
pub fn lower_program(input: &OptInput<'_>) -> R<HirModule> {
    let program = input.program;
    let mut globals = rustc_hash::FxHashMap::default();

    // Pass 1: register every top-level function/`let` name as a global so calls
    // and references resolve regardless of declaration order.
    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            match decl.as_ref() {
                Decl::Function(f) => {
                    globals.insert(f.id.clone(), ());
                }
                Decl::Variable(v) => {
                    for d in &v.declarators {
                        if let Pattern::Identifier { name, .. } = &d.id {
                            globals.insert(name.clone(), ());
                        } else {
                            return unsupported("top-level destructuring binding");
                        }
                    }
                }
                _ => return unsupported("top-level class/enum/interface decl"),
            }
        }
    }

    let mut lo = Lowerer {
        ann: input.annotations,
        globals,
    };

    // Pass 2: lower each declared function and the module top level.
    let mut functions = Vec::new();
    let mut top_body = Vec::new();
    for stmt in &program.body {
        match &stmt.kind {
            StmtKind::Decl(decl) => match decl.as_ref() {
                Decl::Function(f) => functions.push(lo.lower_function(f)?),
                Decl::Variable(v) => {
                    // Top-level `let x = e` -> assign module global x.
                    for d in &v.declarators {
                        let name = match &d.id {
                            Pattern::Identifier { name, .. } => name.clone(),
                            _ => return unsupported("top-level destructuring"),
                        };
                        let value = match &d.init {
                            Some(e) => lo.lower_expr(e, &mut Scope::new())?,
                            None => HirExpr::Null,
                        };
                        top_body.push(HirStmt::Assign {
                            target: HirBinding::Global(name),
                            value,
                        });
                    }
                }
                _ => return unsupported("top-level decl"),
            },
            _ => {
                let mut scope = Scope::new();
                lo.lower_stmt(stmt, &mut scope, &mut top_body)?;
            }
        }
    }

    let top_level = HirFunction {
        name: Rc::from("<module>"),
        params: Vec::new(),
        locals: 0,
        body: top_body,
        return_ty: HirType::Dynamic,
    };

    Ok(HirModule {
        top_level,
        functions,
    })
}

impl<'a> Lowerer<'a> {
    fn lower_function(&mut self, f: &FunctionDecl) -> R<HirFunction> {
        if f.modifiers.is_async || f.modifiers.is_generator {
            return unsupported("async/generator function");
        }
        if !f.type_params.is_empty() {
            return unsupported("generic function");
        }
        let mut scope = Scope::new();
        let mut params = Vec::new();
        for (i, p) in f.params.iter().enumerate() {
            if p.is_rest || p.is_optional || p.default.is_some() {
                return unsupported("rest/optional/default param");
            }
            let name = match &p.pattern {
                Pattern::Identifier { name, .. } => name.clone(),
                _ => return unsupported("destructuring param"),
            };
            scope.define(name.clone(), HirBinding::Param(i as u32));
            params.push(HirParam {
                name,
                ty: param_ty(p),
            });
        }
        let mut body = Vec::new();
        match &f.body.kind {
            StmtKind::Block { stmts } => {
                for s in stmts {
                    self.lower_stmt(s, &mut scope, &mut body)?;
                }
            }
            _ => self.lower_stmt(&f.body, &mut scope, &mut body)?,
        }
        Ok(HirFunction {
            name: f.id.clone(),
            params,
            locals: scope.next_local,
            body,
            return_ty: HirType::Dynamic,
        })
    }

    fn lower_block(&mut self, stmt: &Stmt, scope: &mut Scope) -> R<Vec<HirStmt>> {
        let mut out = Vec::new();
        scope.push();
        match &stmt.kind {
            StmtKind::Block { stmts } => {
                for s in stmts {
                    self.lower_stmt(s, scope, &mut out)?;
                }
            }
            _ => self.lower_stmt(stmt, scope, &mut out)?,
        }
        scope.pop();
        Ok(out)
    }

    fn lower_stmt(&mut self, stmt: &Stmt, scope: &mut Scope, out: &mut Vec<HirStmt>) -> R<()> {
        match &stmt.kind {
            StmtKind::Empty => {}
            StmtKind::Block { stmts } => {
                scope.push();
                for s in stmts {
                    self.lower_stmt(s, scope, out)?;
                }
                scope.pop();
            }
            StmtKind::Expr { expression } => {
                if let ExprKind::Assign { op, target, value } = &expression.kind {
                    let binding = match &target.kind {
                        ExprKind::Identifier { name } => self.resolve(name, scope),
                        _ => return unsupported("non-identifier assign target"),
                    };
                    let val_expr = self.lower_expr(value, scope)?;
                    let value = match op {
                        AssignOp::Assign => val_expr,
                        _ => {
                            let bop = compound_to_bin(*op)?;
                            let ty = numeric_ty(self.ann, expression.range.start.offset);
                            HirExpr::Binary {
                                op: bop,
                                lhs: Box::new(HirExpr::Var(binding.clone())),
                                rhs: Box::new(val_expr),
                                ty,
                            }
                        }
                    };
                    out.push(HirStmt::Assign {
                        target: binding,
                        value,
                    });
                } else {
                    let e = self.lower_expr(expression, scope)?;
                    out.push(HirStmt::Expr(e));
                }
            }
            StmtKind::Decl(decl) => match decl.as_ref() {
                Decl::Variable(v) => {
                    for d in &v.declarators {
                        let name = match &d.id {
                            Pattern::Identifier { name, .. } => name.clone(),
                            _ => return unsupported("destructuring let"),
                        };
                        let value = match &d.init {
                            Some(e) => self.lower_expr(e, scope)?,
                            None => HirExpr::Null,
                        };
                        let local = scope.alloc_local(name);
                        out.push(HirStmt::Let {
                            local,
                            value,
                            ty: HirType::Dynamic,
                        });
                    }
                }
                _ => return unsupported("nested function/class decl"),
            },
            StmtKind::Return { argument } => {
                let v = match argument {
                    Some(e) => Some(self.lower_expr(e, scope)?),
                    None => None,
                };
                out.push(HirStmt::Return(v));
            }
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                let test = self.lower_expr(test, scope)?;
                let then_body = self.lower_block(consequent, scope)?;
                let else_body = match alternate {
                    Some(alt) => self.lower_block(alt, scope)?,
                    None => Vec::new(),
                };
                out.push(HirStmt::If {
                    test,
                    then_body,
                    else_body,
                });
            }
            StmtKind::While { test, body } => {
                let test = self.lower_expr(test, scope)?;
                let body = self.lower_block(body, scope)?;
                out.push(HirStmt::While { test, body });
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                // Desugar `for (init; test; update) body`
                //   -> init; while (test) { body; update }
                scope.push();
                if let Some(init) = init {
                    match init.as_ref() {
                        ForInit::Expr(e) => {
                            let e = self.lower_expr(e, scope)?;
                            out.push(HirStmt::Expr(e));
                        }
                        ForInit::Var { declarators, .. } => {
                            for d in declarators {
                                let name = match &d.id {
                                    Pattern::Identifier { name, .. } => name.clone(),
                                    _ => return unsupported("for-init destructuring"),
                                };
                                let value = match &d.init {
                                    Some(e) => self.lower_expr(e, scope)?,
                                    None => HirExpr::Null,
                                };
                                let local = scope.alloc_local(name);
                                out.push(HirStmt::Let {
                                    local,
                                    value,
                                    ty: HirType::Dynamic,
                                });
                            }
                        }
                    }
                }
                let test = match test {
                    Some(t) => self.lower_expr(t, scope)?,
                    None => HirExpr::Bool(true),
                };
                let mut loop_body = self.lower_block(body, scope)?;
                if let Some(u) = update {
                    let u = self.lower_expr(u, scope)?;
                    loop_body.push(HirStmt::Expr(u));
                }
                out.push(HirStmt::While {
                    test,
                    body: loop_body,
                });
                scope.pop();
            }
            StmtKind::Break { label: None } => out.push(HirStmt::Break),
            StmtKind::Continue { label: None } => out.push(HirStmt::Continue),
            _ => return unsupported("statement kind"),
        }
        Ok(())
    }

    fn lower_expr(&mut self, expr: &Expr, scope: &mut Scope) -> R<HirExpr> {
        let offset = expr.range.start.offset;
        match &expr.kind {
            ExprKind::IntLiteral { value, .. } => Ok(HirExpr::Int(*value)),
            ExprKind::FloatLiteral { value, .. } => Ok(HirExpr::Float(*value)),
            ExprKind::StrLiteral { value } => Ok(HirExpr::Str(Rc::from(value.as_str()))),
            ExprKind::BoolLiteral { value } => Ok(HirExpr::Bool(*value)),
            ExprKind::NullLiteral => Ok(HirExpr::Null),
            ExprKind::Paren { expression } => self.lower_expr(expression, scope),
            ExprKind::Identifier { name } => Ok(HirExpr::Var(self.resolve(name, scope))),
            ExprKind::Binary { op, left, right } => {
                let lhs = Box::new(self.lower_expr(left, scope)?);
                let rhs = Box::new(self.lower_expr(right, scope)?);
                let ty = numeric_ty(self.ann, offset);
                Ok(HirExpr::Binary {
                    op: bin_op(*op)?,
                    lhs,
                    rhs,
                    ty,
                })
            }
            ExprKind::Unary { op, operand, .. } => {
                let operand = Box::new(self.lower_expr(operand, scope)?);
                Ok(HirExpr::Unary {
                    op: un_op(*op)?,
                    operand,
                    ty: HirType::Dynamic,
                })
            }
            ExprKind::Call {
                callee,
                args,
                optional,
                ..
            } => {
                if *optional {
                    return unsupported("optional call");
                }
                // Only simple positional calls (no named/spread/defaults).
                if self.ann.get_intrinsic(offset).is_some() {
                    return unsupported("intrinsic call");
                }
                let mut hargs = Vec::with_capacity(args.len());
                for a in args {
                    match a {
                        Arg::Positional(e) => hargs.push(self.lower_expr(e, scope)?),
                        _ => return unsupported("named/spread arg"),
                    }
                }
                let callee = Box::new(self.lower_expr(callee, scope)?);
                Ok(HirExpr::Call {
                    callee,
                    args: hargs,
                    ty: HirType::Dynamic,
                })
            }
            // Member/index access compiles to GetProperty/GetIndex, which need
            // IC cache slots — deferred to a later stage; fall back for now.
            ExprKind::Member { .. } => unsupported("member access"),
            _ => unsupported("expression kind"),
        }
    }

    fn resolve(&self, name: &Rc<str>, scope: &Scope) -> HirBinding {
        if let Some(b) = scope.resolve(name) {
            b
        } else {
            // Unresolved locally -> module global (covers builtins like `print`
            // too, which the VM resolves by name).
            HirBinding::Global(name.clone())
        }
    }
}

fn param_ty(_p: &Param) -> HirType {
    HirType::Dynamic
}

fn numeric_ty(ann: &TypeAnnotations, offset: u32) -> HirType {
    match ann.get_numeric(offset) {
        Some(NumericKind::Int) => HirType::Int,
        Some(NumericKind::Float) => HirType::Float,
        _ => HirType::Dynamic,
    }
}

fn bin_op(op: BinaryOp) -> R<HirBinOp> {
    Ok(match op {
        BinaryOp::Add => HirBinOp::Add,
        BinaryOp::Sub => HirBinOp::Sub,
        BinaryOp::Mul => HirBinOp::Mul,
        BinaryOp::Div => HirBinOp::Div,
        BinaryOp::Mod => HirBinOp::Mod,
        BinaryOp::Pow => HirBinOp::Pow,
        BinaryOp::Eq => HirBinOp::Eq,
        BinaryOp::NotEq => HirBinOp::Ne,
        BinaryOp::Lt => HirBinOp::Lt,
        BinaryOp::LtEq => HirBinOp::Le,
        BinaryOp::Gt => HirBinOp::Gt,
        BinaryOp::GtEq => HirBinOp::Ge,
        BinaryOp::BitAnd => HirBinOp::BitAnd,
        BinaryOp::BitOr => HirBinOp::BitOr,
        BinaryOp::BitXor => HirBinOp::BitXor,
        BinaryOp::Shl => HirBinOp::Shl,
        BinaryOp::Shr => HirBinOp::Shr,
        _ => return unsupported("binary op"),
    })
}

fn compound_to_bin(op: AssignOp) -> R<HirBinOp> {
    Ok(match op {
        AssignOp::AddAssign => HirBinOp::Add,
        AssignOp::SubAssign => HirBinOp::Sub,
        AssignOp::MulAssign => HirBinOp::Mul,
        AssignOp::DivAssign => HirBinOp::Div,
        AssignOp::ModAssign => HirBinOp::Mod,
        AssignOp::BitAndAssign => HirBinOp::BitAnd,
        AssignOp::BitOrAssign => HirBinOp::BitOr,
        AssignOp::BitXorAssign => HirBinOp::BitXor,
        AssignOp::ShlAssign => HirBinOp::Shl,
        AssignOp::ShrAssign => HirBinOp::Shr,
        _ => return unsupported("compound assignment op"),
    })
}

fn un_op(op: UnaryOp) -> R<HirUnOp> {
    Ok(match op {
        UnaryOp::Minus => HirUnOp::Neg,
        UnaryOp::Not => HirUnOp::Not,
        _ => return unsupported("unary op"),
    })
}
