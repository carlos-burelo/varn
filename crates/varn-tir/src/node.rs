//! The nodes.
//!
//! `ty` and `res` are fields of `TirExpr`, so a constructor cannot omit them.
//! That is the whole point: in `HirExpr` the type was a field of *some*
//! variants, and the ones without it — Array, Object, Assign, OptionalChain,
//! TryOp — left the consumer to assume.

use crate::resolution::Resolution;
use crate::ty::{BackendTy, ClassId, FnId};
use std::sync::Arc;

/// Byte range in the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const EMPTY: Span = Span { start: 0, end: 0 };
}

/// An expression: what it does, what it produces, what it resolves against.
#[derive(Debug, Clone)]
pub struct TirExpr {
    pub kind: TirExprKind,
    pub ty: BackendTy,
    pub res: Resolution,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TirBinOp {
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
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Ushr,
    /// `x instanceof C` — always produces `Bool`, operands are references.
    Instanceof,
    /// `k in obj` — membership, always `Bool`.
    In,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TirUnOp {
    Neg,
    Not,
    BitNot,
    /// `typeof x` — yields the runtime type name as a string.
    Typeof,
    /// The null test `?.` and `??` desugar against. The emitter binds the
    /// tested value to a temp local, then branches on `IsNull(Var)` inside a
    /// `Select` — that is the whole short-circuit, no dedicated node.
    IsNull,
}

/// One entry in an argument list or an array literal. `Spread` is a position,
/// not an operation, so it is not a `TirExprKind`: it can only appear here.
#[derive(Debug, Clone)]
pub enum TirArg {
    Expr(TirExpr),
    Spread(TirExpr),
    Named { label: Arc<str>, value: TirExpr },
}

impl TirArg {
    pub fn value(&self) -> &TirExpr {
        match self {
            TirArg::Expr(e) | TirArg::Spread(e) | TirArg::Named { value: e, .. } => e,
        }
    }
    pub fn is_spread(&self) -> bool {
        matches!(self, TirArg::Spread(_))
    }
}

/// One element of an array literal — a value, a spread, or a hole (`[1, , 3]`).
#[derive(Debug, Clone)]
pub enum TirArrayEl {
    Expr(TirExpr),
    Spread(TirExpr),
    Hole,
}

/// One entry of an object literal.
#[derive(Debug, Clone)]
pub enum TirObjectEntry {
    Field { name: Arc<str>, value: TirExpr },
    Spread(TirExpr),
}

#[derive(Debug, Clone)]
pub enum TirExprKind {
    // Literals
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),
    StrLit(Arc<str>),
    CharLit(char),
    NullLit,

    /// A binding reference. Which binding is in `res`.
    Var,

    Binary {
        op: TirBinOp,
        lhs: Box<TirExpr>,
        rhs: Box<TirExpr>,
    },
    Unary {
        op: TirUnOp,
        operand: Box<TirExpr>,
    },

    /// Field access. Slot or by-name lives in `res`, not in the kind.
    Field {
        object: Box<TirExpr>,
        name: Arc<str>,
    },
    Index {
        object: Box<TirExpr>,
        index: Box<TirExpr>,
    },

    Call {
        callee: Box<TirExpr>,
        args: Vec<TirArg>,
    },
    /// Method call. Vtable slot, intrinsic or by-name lives in `res`.
    MethodCall {
        recv: Box<TirExpr>,
        name: Arc<str>,
        args: Vec<TirArg>,
    },

    Assign {
        target: Box<TirExpr>,
        value: Box<TirExpr>,
    },

    ArrayLit(Vec<TirArrayEl>),
    TupleLit(Vec<TirExpr>),
    ObjectLit {
        entries: Vec<TirObjectEntry>,
    },
    /// `#{ k: v, … }` — a deeply-immutable record; `==` on it is structural.
    RecordLit {
        fields: Vec<(Arc<str>, TirExpr)>,
    },

    /// `await e` — only legal in a function whose `is_async` is set, which the
    /// verifier enforces. No suspension state in the IR: the backend builds the
    /// state machine, exactly as it does from HIR today.
    Await {
        future: Box<TirExpr>,
    },
    /// `yield e` / `yield* e` — only legal when `is_generator` is set.
    Yield {
        value: Option<Box<TirExpr>>,
        delegate: bool,
    },

    /// The integer tag of an enum value. `match` lowers to a chain of `If`s
    /// comparing this against constants.
    Discriminant {
        value: Box<TirExpr>,
    },
    /// One payload field of an enum value, once the tag is known. `res` is the
    /// `EnumVariant` it was narrowed to.
    VariantPayload {
        value: Box<TirExpr>,
        tag: u16,
        field: u16,
    },
    /// `e is T` — and the primitive a type pattern lowers to. Produces `Bool`.
    TypeTest {
        value: Box<TirExpr>,
        class: ClassId,
    },

    /// Explicit representation change. The verifier requires one wherever an
    /// operation would otherwise mix representations.
    Cast {
        operand: Box<TirExpr>,
    },

    /// A function value. `func` is the `TirFunction` holding its body; captures
    /// are resolved by the backend against the parent frame, as HIR does.
    /// A function value. `func` holds the body; `upvalues` says, in upvalue-
    /// index order, where each captured value comes from in the ENCLOSING
    /// frame — the backend needs this to build the closure record.
    Closure {
        func: FnId,
        upvalues: Vec<TirUpvalue>,
    },

    /// Construction of a class instance.
    New {
        class: ClassId,
        args: Vec<TirArg>,
    },
    /// Construction of an enum variant. Which variant lives in `res`.
    MakeVariant {
        args: Vec<TirArg>,
    },

    /// `cond ? a : b`
    Select {
        cond: Box<TirExpr>,
        then_val: Box<TirExpr>,
        else_val: Box<TirExpr>,
    },
    Seq {
        stmts: Vec<TirStmt>,
        value: Box<TirExpr>,
    },

    /// The enumerable string keys of an object — the iterand of `for…in`.
    /// Produces `str[]`.
    ObjectKeys {
        operand: Box<TirExpr>,
    },
    /// The iterator object for `for…of` — `source[Symbol.iterator]()` (or
    /// `Symbol.asyncIterator` when `is_async`). Works for arrays, generators,
    /// and any object carrying the symbol; the emitter then drives `.next()`.
    IterInit {
        source: Box<TirExpr>,
        is_async: bool,
    },

    /// `super(args)` — the base constructor call, only valid inside a
    /// subclass constructor.
    SuperCall {
        args: Vec<TirArg>,
    },
    /// `super.name(args)` — a base method call bypassing the vtable.
    SuperMethodCall {
        name: Arc<str>,
        args: Vec<TirArg>,
    },

    /// A `decimal` literal, carried as its source text (minus the `d` suffix)
    /// — the backend parses it, keeping this crate free of a bignum dependency.
    DecimalLit(Arc<str>),
    /// A `bigint` literal as canonical base-10 digits (arbitrary precision;
    /// the TIR carries no bignum dependency).
    BigIntLit(Arc<str>),
    /// `a..b` / `a..=b`.
    RangeLit {
        start: Box<TirExpr>,
        end: Box<TirExpr>,
        inclusive: bool,
    },

    /// `const { a, ...rest } = obj` — a shallow copy of `object` without
    /// `skip_keys`.
    ObjectRest {
        object: Box<TirExpr>,
        skip_keys: Vec<Arc<str>>,
    },

    /// `recv.m(args)` resolved to an extension function: a free-function call
    /// with `recv` prepended, dispatched by the mangled `func` name.
    ExtensionCall {
        func: Arc<str>,
        recv: Box<TirExpr>,
        args: Vec<TirArg>,
    },
}

#[derive(Debug, Clone)]
pub enum TirStmt {
    Expr(TirExpr),
    /// A local binding. Its type is the declared type; the initializer's type
    /// must be assignable to it, which the verifier checks.
    Let {
        local: crate::ty::LocalId,
        ty: BackendTy,
        init: Option<TirExpr>,
    },
    Return(Option<TirExpr>),
    If {
        cond: TirExpr,
        then_body: Vec<TirStmt>,
        else_body: Vec<TirStmt>,
    },
    /// The only loop form. `for…of` and `for` are desugared into it by the
    /// emitter — if either survives as its own node, the TIR is not desugared
    /// and `hir/` cannot be deleted.
    Loop {
        cond: TirExpr,
        body: Vec<TirStmt>,
    },
    Break,
    Continue,
    Throw(TirExpr),
    Try {
        body: Vec<TirStmt>,
        catch_local: crate::ty::LocalId,
        catch_body: Vec<TirStmt>,
    },
    /// Build the class/enum at `TirModule::class_defs[n]` and bind its global —
    /// emitted at the declaration's source position so decorators and static
    /// initializers see the module state that precedes it.
    BuildClass(u32),
}

/// Where a closure upvalue is sourced from in the enclosing frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TirUpvalue {
    ParentLocal(u32),
    ParentParam(u32),
    ParentUpvalue(u32),
}
