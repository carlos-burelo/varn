//! The nodes.
//!
//! `ty` and `res` are fields of `TirExpr`, so a constructor cannot omit them.
//! That is the whole point: in `HirExpr` the type was a field of *some*
//! variants, and the ones without it — Array, Object, Assign, OptionalChain,
//! TryOp — left the consumer to assume.

use crate::resolution::Resolution;
use crate::ty::{BackendTy, ClassId, EnumId, FnId, SigId, TyTable};
use std::rc::Rc;

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
    Add, Sub, Mul, Div, Mod, Pow,
    Eq, Ne, Lt, Le, Gt, Ge,
    BitAnd, BitOr, BitXor, Shl, Shr, Ushr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TirUnOp {
    Neg,
    Not,
    BitNot,
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
    Named { label: Rc<str>, value: TirExpr },
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
    Field { name: Rc<str>, value: TirExpr },
    Spread(TirExpr),
}

#[derive(Debug, Clone)]
pub enum TirExprKind {
    // Literals
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),
    StrLit(Rc<str>),
    CharLit(char),
    NullLit,

    /// A binding reference. Which binding is in `res`.
    Var,

    Binary { op: TirBinOp, lhs: Box<TirExpr>, rhs: Box<TirExpr> },
    Unary { op: TirUnOp, operand: Box<TirExpr> },

    /// Field access. Slot or by-name lives in `res`, not in the kind.
    Field { object: Box<TirExpr>, name: Rc<str> },
    Index { object: Box<TirExpr>, index: Box<TirExpr> },

    Call { callee: Box<TirExpr>, args: Vec<TirArg> },
    /// Method call. Vtable slot, intrinsic or by-name lives in `res`.
    MethodCall { recv: Box<TirExpr>, name: Rc<str>, args: Vec<TirArg> },

    Assign { target: Box<TirExpr>, value: Box<TirExpr> },

    ArrayLit(Vec<TirArrayEl>),
    TupleLit(Vec<TirExpr>),
    ObjectLit { entries: Vec<TirObjectEntry> },

    /// `await e` — only legal in a function whose `is_async` is set, which the
    /// verifier enforces. No suspension state in the IR: the backend builds the
    /// state machine, exactly as it does from HIR today.
    Await { future: Box<TirExpr> },
    /// `yield e` / `yield* e` — only legal when `is_generator` is set.
    Yield { value: Option<Box<TirExpr>>, delegate: bool },

    /// The integer tag of an enum value. `match` lowers to a chain of `If`s
    /// comparing this against constants.
    Discriminant { value: Box<TirExpr> },
    /// One payload field of an enum value, once the tag is known. `res` is the
    /// `EnumVariant` it was narrowed to.
    VariantPayload { value: Box<TirExpr>, tag: u16, field: u16 },
    /// `e is T` — and the primitive a type pattern lowers to. Produces `Bool`.
    TypeTest { value: Box<TirExpr>, class: ClassId },

    /// Explicit representation change. The verifier requires one wherever an
    /// operation would otherwise mix representations.
    Cast { operand: Box<TirExpr> },

    /// Construction of a class instance.
    New { class: ClassId, args: Vec<TirArg> },
    /// Construction of an enum variant. Which variant lives in `res`.
    MakeVariant { args: Vec<TirArg> },

    /// `cond ? a : b`
    Select { cond: Box<TirExpr>, then_val: Box<TirExpr>, else_val: Box<TirExpr> },
}

#[derive(Debug, Clone)]
pub enum TirStmt {
    Expr(TirExpr),
    /// A local binding. Its type is the declared type; the initializer's type
    /// must be assignable to it, which the verifier checks.
    Let { local: crate::ty::LocalId, ty: BackendTy, init: Option<TirExpr> },
    Return(Option<TirExpr>),
    If { cond: TirExpr, then_body: Vec<TirStmt>, else_body: Vec<TirStmt> },
    /// The only loop form. `for…of` and `for` are desugared into it by the
    /// emitter — if either survives as its own node, the TIR is not desugared
    /// and `hir/` cannot be deleted.
    Loop { cond: TirExpr, body: Vec<TirStmt> },
    Break,
    Continue,
    Throw(TirExpr),
    Try { body: Vec<TirStmt>, catch_local: crate::ty::LocalId, catch_body: Vec<TirStmt> },
}

#[derive(Debug, Clone)]
pub struct TirFunction {
    pub name: Rc<str>,
    pub sig: SigId,
    pub params: Vec<BackendTy>,
    pub return_ty: BackendTy,
    pub locals: Vec<BackendTy>,
    pub body: Vec<TirStmt>,
    pub has_this: bool,
    pub this_class: Option<ClassId>,
    /// Declared shape, propagated from the source. Gates `Await` / `Yield` in
    /// the verifier and tells the backend to build a state machine.
    pub is_async: bool,
    pub is_generator: bool,
}

#[derive(Debug)]
pub struct TirModule {
    pub source_file: Rc<str>,
    pub types: TyTable,
    pub classes: Vec<crate::tables::ClassInfo>,
    pub enums: Vec<crate::tables::EnumInfo>,
    pub signatures: Vec<crate::tables::Signature>,
    pub functions: Vec<TirFunction>,
    pub globals: Vec<BackendTy>,
    pub top_level: TirFunction,
}

impl TirModule {
    pub fn class(&self, id: ClassId) -> Option<&crate::tables::ClassInfo> {
        self.classes.get(id.0 as usize)
    }
    pub fn enum_info(&self, id: EnumId) -> Option<&crate::tables::EnumInfo> {
        self.enums.get(id.0 as usize)
    }
    pub fn signature(&self, id: SigId) -> Option<&crate::tables::Signature> {
        self.signatures.get(id.0 as usize)
    }
    pub fn function(&self, id: FnId) -> Option<&TirFunction> {
        self.functions.get(id.0 as usize)
    }
}
