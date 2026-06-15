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

/// Where an upvalue captured by a closure comes from, in terms of the *parent*
/// function's bindings. Registers are not assigned until bytecode lowering, so
/// the capture is described symbolically here and resolved to a `(is_local,
/// index)` pair (the `MakeClosure` encoding) by the lowerer, which knows the
/// parent's register layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirUpvalueSrc {
    /// A local in the immediate parent (is_local = true).
    ParentLocal(LocalId),
    /// A parameter of the immediate parent (is_local = true).
    ParentParam(u32),
    /// An upvalue of the immediate parent, by parent-upvalue index
    /// (is_local = false).
    ParentUpvalue(u32),
}

/// A binding target whose register is only known at bytecode-lowering time;
/// used to compute the lowest captured register for `CloseUpvalue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTarget {
    Param(u32),
    Local(LocalId),
}

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

/// Short-circuiting logical operators (lowered with branches, not as `Binary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirLogicalOp {
    And,
    Or,
    Nullish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirUpdateOp {
    Inc,
    Dec,
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
    /// Short-circuiting `&&`/`||`/`??`. Lowered with branches + `Move`, so it
    /// cannot be a `Binary`.
    Logical {
        op: HirLogicalOp,
        lhs: Box<HirExpr>,
        rhs: Box<HirExpr>,
    },
    /// Ternary `test ? cons : alt`.
    Conditional {
        test: Box<HirExpr>,
        cons: Box<HirExpr>,
        alt: Box<HirExpr>,
    },
    /// `++`/`--` on a simple binding. `prefix` selects whether the expression
    /// yields the new (prefix) or old (postfix) value.
    Update {
        target: HirBinding,
        op: HirUpdateOp,
        prefix: bool,
    },
    /// Array literal with only plain element expressions (no spread/holes).
    Array(Vec<HirExpr>),
    /// Object literal with a fixed shape: all-static keys, plain value props
    /// (no computed keys, methods, getters/setters, or spreads).
    Object {
        keys: Vec<Rc<str>>,
        values: Vec<HirExpr>,
    },
    /// A function/arrow expression or a nested function declaration captured as
    /// a first-class value. Lowers to `MakeClosure` (or `LoadStaticFn` when
    /// `upvalues` is empty). The nested function is owned inline.
    Closure {
        func: Box<HirFunction>,
        upvalues: Vec<HirUpvalueSrc>,
    },
    /// The method receiver, register 0 in a `has_this` function.
    This,
    /// A class value: `MakeClass` + `DeclareField`(s) + `Method`(s). Bound to a
    /// global or local by the caller. Core subset: no inheritance/static/
    /// accessors/decorators.
    Class(Box<HirClass>),
}

/// A class method (or constructor) plus the upvalues its closure captures.
#[derive(Debug, Clone)]
pub struct HirMethod {
    pub key: Rc<str>,
    pub func: HirFunction,
    pub upvalues: Vec<HirUpvalueSrc>,
}

#[derive(Debug, Clone)]
pub struct HirClass {
    pub name: Rc<str>,
    /// Instance field names (`DeclareField`); initializers live in `ctor`.
    pub fields: Vec<Rc<str>>,
    /// The constructor (synthesised if the source omits one). Runs field
    /// initializers after its body, matching legacy.
    pub ctor: HirMethod,
    pub methods: Vec<HirMethod>,
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
    /// `object.name = value` → `SetProperty` (with an IC slot).
    SetMember {
        object: HirExpr,
        name: Rc<str>,
        value: HirExpr,
    },
    /// `object[index] = value` → `SetIndex`.
    SetIndex {
        object: HirExpr,
        index: HirExpr,
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
    /// Close any open upvalues that point at the given capture targets' slots,
    /// emitted when a block/function that declared captured bindings is about
    /// to go out of scope. Lowers to `CloseUpvalue` at the lowest such register.
    CloseUpvalues(Vec<CaptureTarget>),
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
    /// Number of upvalues this function captures (sets `FunctionProto.
    /// upvalue_count`). Zero for top-level/module functions.
    pub upvalue_count: u32,
    /// Whether register 0 is a meaningful receiver (`this`) — true for methods
    /// and constructors. Sets `FunctionProto.has_this`.
    pub has_this: bool,
}

/// A whole module: the synthetic top-level function plus the functions it
/// declares.
#[derive(Debug, Clone)]
pub struct HirModule {
    pub top_level: HirFunction,
    pub functions: Vec<HirFunction>,
}
