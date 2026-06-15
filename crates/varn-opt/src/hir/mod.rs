//! HIR — a typed, desugared, control-flow-explicit intermediate representation.
//!
//! Lowered from the AST: names are resolved (param/local/global/upvalue),
//! syntactic sugar is removed, and types from the checker's `TypeAnnotations`
//! are attached. SSA construction (`crate::ssa`) consumes HIR; the naive
//! `crate::lower` path can also emit bytecode straight from HIR for bring-up.
//!
//! The shapes below are the Stage 1 starting set and will grow with the
//! supported subset.

#![allow(dead_code)]

use std::rc::Rc;

pub mod lower;

/// Static representation class for a value, carried from the checker so opt
/// passes and lowering can specialise without runtime guards. Mirrors
/// `varn_types::register_meta::SlotKind` intent at the HIR level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirType {
    Int,
    Float,
    Bool,
    Str,
    /// Heap reference (object/array/closure/…); not further refined yet.
    Ref,
    /// Statically unknown — behaves like the legacy `Dynamic` slot.
    Dynamic,
}

/// A resolved binding the front-end has classified.
#[derive(Debug, Clone)]
pub enum HirBinding {
    /// Function parameter, by index.
    Param(u32),
    /// Local variable, by a function-unique id.
    Local(LocalId),
    /// Module global, by name (resolved to an index during lowering).
    Global(Rc<str>),
    /// Captured upvalue, by index.
    Upvalue(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirUnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub enum HirExpr {
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Bool(bool),
    Null,
    /// Reference to a resolved binding.
    Var(HirBinding),
    Binary {
        op: HirBinOp,
        lhs: Box<HirExpr>,
        rhs: Box<HirExpr>,
        ty: HirType,
    },
    Unary {
        op: HirUnOp,
        operand: Box<HirExpr>,
        ty: HirType,
    },
    /// Direct call of a callee expression with positional args.
    Call {
        callee: Box<HirExpr>,
        args: Vec<HirExpr>,
        ty: HirType,
    },
    /// Statically-resolved self-recursion: a call of the enclosing function by
    /// its own (non-reassigned, non-shadowed) name. Lowers to `CallSelf`, which
    /// the JIT turns into a direct in-machine-code recursive call instead of a
    /// VM re-entry. Mirrors legacy `can_emit_self_call`/`emit_self_call`.
    SelfCall {
        args: Vec<HirExpr>,
        ty: HirType,
    },
    /// `object.name` (non-computed member access).
    Member {
        object: Box<HirExpr>,
        name: Rc<str>,
        ty: HirType,
    },
    /// `object[index]` (computed access).
    Index {
        object: Box<HirExpr>,
        index: Box<HirExpr>,
        ty: HirType,
    },
    /// `recv.name(args)` — a method call on a non-computed property. Lowers to
    /// `CallMethod` with an inline-cache slot, distinct from `Call` because the
    /// receiver is bound as `this` without a separate callee load.
    MethodCall {
        recv: Box<HirExpr>,
        name: Rc<str>,
        args: Vec<HirExpr>,
        ty: HirType,
    },
}

#[derive(Debug, Clone)]
pub enum HirStmt {
    Expr(HirExpr),
    /// Bind a new local to a value.
    Let {
        local: LocalId,
        value: HirExpr,
        ty: HirType,
    },
    /// Assign to an existing binding.
    Assign {
        target: HirBinding,
        value: HirExpr,
    },
    Return(Option<HirExpr>),
    If {
        test: HirExpr,
        then_body: Vec<HirStmt>,
        else_body: Vec<HirStmt>,
    },
    /// Pre-tested loop (`while`, and the desugaring target of `for`).
    While {
        test: HirExpr,
        body: Vec<HirStmt>,
    },
    Break,
    Continue,
}

#[derive(Debug, Clone)]
pub struct HirParam {
    pub name: Rc<str>,
    pub ty: HirType,
}

#[derive(Debug, Clone)]
pub struct HirFunction {
    pub name: Rc<str>,
    pub params: Vec<HirParam>,
    pub locals: u32,
    pub body: Vec<HirStmt>,
    pub return_ty: HirType,
}

/// A whole module: the synthetic top-level function plus the functions it
/// declares.
#[derive(Debug, Clone)]
pub struct HirModule {
    pub top_level: HirFunction,
    pub functions: Vec<HirFunction>,
}
