use crate::resolution::Resolution;
use crate::ty::{BackendTy, ClassId, FnId};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const EMPTY: Span = Span { start: 0, end: 0 };
}

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

    Instanceof,

    In,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TirUnOp {
    Neg,
    Not,
    BitNot,

    Typeof,

    IsNull,
}

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

#[derive(Debug, Clone)]
pub enum TirArrayEl {
    Expr(TirExpr),
    Spread(TirExpr),
    Hole,
}

#[derive(Debug, Clone)]
pub enum TirObjectEntry {
    Field { name: Arc<str>, value: TirExpr },
    Spread(TirExpr),
}

#[derive(Debug, Clone)]
pub enum TirExprKind {
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),
    StrLit(Arc<str>),
    CharLit(char),
    NullLit,

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

    RecordLit {
        fields: Vec<(Arc<str>, TirExpr)>,
    },

    Await {
        future: Box<TirExpr>,
    },

    Yield {
        value: Option<Box<TirExpr>>,
        delegate: bool,
    },

    Discriminant {
        value: Box<TirExpr>,
    },

    VariantPayload {
        value: Box<TirExpr>,
        tag: u16,
        field: u16,
    },

    TypeTest {
        value: Box<TirExpr>,
        class: ClassId,
    },

    Cast {
        operand: Box<TirExpr>,
    },

    Closure {
        func: FnId,
        upvalues: Vec<TirUpvalue>,
    },

    New {
        class: ClassId,
        args: Vec<TirArg>,
    },

    MakeVariant {
        args: Vec<TirArg>,
    },

    Select {
        cond: Box<TirExpr>,
        then_val: Box<TirExpr>,
        else_val: Box<TirExpr>,
    },
    Seq {
        stmts: Vec<TirStmt>,
        value: Box<TirExpr>,
    },

    ObjectKeys {
        operand: Box<TirExpr>,
    },

    IterInit {
        source: Box<TirExpr>,
        is_async: bool,
    },

    SuperCall {
        args: Vec<TirArg>,
    },

    SuperMethodCall {
        name: Arc<str>,
        args: Vec<TirArg>,
    },

    DecimalLit(Arc<str>),

    BigIntLit(Arc<str>),

    RangeLit {
        start: Box<TirExpr>,
        end: Box<TirExpr>,
        inclusive: bool,
    },

    ObjectRest {
        object: Box<TirExpr>,
        skip_keys: Vec<Arc<str>>,
    },

    ExtensionCall {
        func: Arc<str>,
        recv: Box<TirExpr>,
        args: Vec<TirArg>,
    },
}

#[derive(Debug, Clone)]
pub enum TirStmt {
    Expr(TirExpr),

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

    BuildClass(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TirUpvalue {
    ParentLocal(u32),
    ParentParam(u32),
    ParentUpvalue(u32),
}
