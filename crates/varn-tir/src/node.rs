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

    Call { callee: Box<TirExpr>, args: Vec<TirExpr> },
    /// Method call. Vtable slot, intrinsic or by-name lives in `res`.
    MethodCall { recv: Box<TirExpr>, name: Rc<str>, args: Vec<TirExpr> },

    Assign { target: Box<TirExpr>, value: Box<TirExpr> },

    ArrayLit(Vec<TirExpr>),
    TupleLit(Vec<TirExpr>),
    ObjectLit { fields: Vec<(Rc<str>, TirExpr)> },

    /// Explicit representation change. The verifier requires one wherever an
    /// operation would otherwise mix representations.
    Cast { operand: Box<TirExpr> },

    /// Construction of a class instance.
    New { class: ClassId, args: Vec<TirExpr> },
    /// Construction of an enum variant. Which variant lives in `res`.
    MakeVariant { args: Vec<TirExpr> },

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
