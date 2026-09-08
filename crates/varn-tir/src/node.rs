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

    /// A function value. `func` is the `TirFunction` holding its body; captures
    /// are resolved by the backend against the parent frame, as HIR does.
    /// A function value. `func` holds the body; `upvalues` says, in upvalue-
    /// index order, where each captured value comes from in the ENCLOSING
    /// frame — the backend needs this to build the closure record.
    Closure { func: FnId, upvalues: Vec<TirUpvalue> },

    /// Construction of a class instance.
    New { class: ClassId, args: Vec<TirArg> },
    /// Construction of an enum variant. Which variant lives in `res`.
    MakeVariant { args: Vec<TirArg> },

    /// `cond ? a : b`
    Select { cond: Box<TirExpr>, then_val: Box<TirExpr>, else_val: Box<TirExpr> },

    /// The enumerable string keys of an object — the iterand of `for…in`.
    /// Produces `str[]`.
    ObjectKeys { operand: Box<TirExpr> },
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

/// Where a closure upvalue is sourced from in the enclosing frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TirUpvalue {
    ParentLocal(u32),
    ParentParam(u32),
    ParentUpvalue(u32),
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

/// Everything the backend needs to BUILD a class/enum object at module load
/// and bind it to its global. The `classes` / `enums` tables describe layout
/// and dispatch; this describes construction. Instance-field names and types
/// come from the referenced table entry, not repeated here.
#[derive(Debug, Clone, Default)]
pub struct TirClassDef {
    pub name: Rc<str>,
    /// Table handle: `Some(Ok)` a class, `Some(Err)` an enum, `None` neither
    /// resolved (a generic-only or erased declaration — still built by name).
    pub class_id: Option<ClassId>,
    pub enum_id: Option<EnumId>,
    /// Hoisted temporaries from `super_class` / decorator / static-init
    /// expressions, emitted before the `MakeClass`.
    pub prelude: Vec<TirStmt>,
    /// The `extends` expression, evaluated for the `MakeClass` super argument.
    pub super_class: Option<TirExpr>,
    /// Static fields / consts: name + optional initializer.
    pub statics: Vec<(Rc<str>, Option<TirExpr>)>,
    /// Methods and the constructor: key, body `FnId`, `is_static`.
    pub methods: Vec<TirClassMember>,
    /// Getters / setters: key, body `FnId`, `is_getter`, `is_static`.
    pub accessors: Vec<TirClassAccessor>,
    /// Class decorators, applied outermost-last.
    pub decorators: Vec<TirExpr>,
    /// `static { ... }` blocks, as `FnId`s to invoke after the class is bound.
    pub static_blocks: Vec<FnId>,
    /// Enum variants: name, tag, metadata string, payload default args.
    pub variants: Vec<TirVariantDef>,
}

#[derive(Debug, Clone)]
pub struct TirClassMember {
    pub key: Rc<str>,
    pub func: FnId,
    pub is_static: bool,
}

#[derive(Debug, Clone)]
pub struct TirClassAccessor {
    pub key: Rc<str>,
    pub func: FnId,
    pub is_getter: bool,
    pub is_static: bool,
}

#[derive(Debug, Clone)]
pub struct TirVariantDef {
    pub name: Rc<str>,
    pub tag: i64,
    pub meta: Rc<str>,
    pub const_args: Vec<TirExpr>,
}

#[derive(Debug, Clone)]
pub enum TirImportKind {
    Default,
    Named(Rc<str>),
    Namespace,
}

#[derive(Debug, Clone)]
pub struct TirImportSpec {
    pub local: Rc<str>,
    pub kind: TirImportKind,
}

/// One `import ... from "src"` — the linkage the backend turns into a
/// `LoadModule` plus a `StoreGlobal` per bound name.
#[derive(Debug, Clone)]
pub struct TirImport {
    pub source: Rc<str>,
    pub is_type_only: bool,
    pub specs: Vec<TirImportSpec>,
}

#[derive(Debug)]
pub struct TirModule {
    pub source_file: Rc<str>,
    pub imports: Vec<TirImport>,
    pub types: TyTable,
    pub classes: Vec<crate::tables::ClassInfo>,
    pub enums: Vec<crate::tables::EnumInfo>,
    pub signatures: Vec<crate::tables::Signature>,
    pub functions: Vec<TirFunction>,
    pub globals: Vec<BackendTy>,
    /// The name of each global, parallel to `globals`. A `GlobalSlot(n)`
    /// resolution names `globals[n]` / `global_names[n]`.
    pub global_names: Vec<Rc<str>>,
    /// Class / enum construction, one per top-level declaration, in source
    /// order. Empty for a module with no classes or enums.
    pub class_defs: Vec<TirClassDef>,
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
