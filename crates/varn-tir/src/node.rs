//! The nodes.
//!
//! `ty` and `res` are fields of `TirExpr`, so a constructor cannot omit them.
//! That is the whole point: in `HirExpr` the type was a field of *some*
//! variants, and the ones without it — Array, Object, Assign, OptionalChain,
//! TryOp — left the consumer to assume.

use crate::resolution::Resolution;
use crate::ty::{BackendTy, ClassId, EnumId, FnId, SigId, TyTable};
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
    /// — the backend parses it, keeping this crate free of `rust_decimal`.
    DecimalLit(Arc<str>),
    /// A `bigint` literal, already parsed to `i128` by the checker.
    BigIntLit(i128),
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

#[derive(Debug, Clone)]
pub struct TirFunction {
    pub name: Arc<str>,
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
    /// The last parameter is `...rest`: the VM packs the trailing arguments
    /// into an array bound to it.
    pub has_rest: bool,
}

/// Everything the backend needs to BUILD a class/enum object at module load
/// and bind it to its global. The `classes` / `enums` tables describe layout
/// and dispatch; this describes construction. Instance-field names and types
/// come from the referenced table entry, not repeated here.
#[derive(Debug, Clone, Default)]
pub struct TirClassDef {
    pub name: Arc<str>,
    /// Table handle: `Some(Ok)` a class, `Some(Err)` an enum, `None` neither
    /// resolved (a generic-only or erased declaration — still built by name).
    pub class_id: Option<ClassId>,
    pub enum_id: Option<EnumId>,
    /// The base class, resolved from `extends` — more reliable than
    /// `ClassInfo::parent`, which the binder sometimes leaves unset.
    pub parent: Option<ClassId>,
    /// Hoisted temporaries from `super_class` / decorator / static-init
    /// expressions, emitted before the `MakeClass`.
    pub prelude: Vec<TirStmt>,
    /// The `extends` expression, evaluated for the `MakeClass` super argument.
    pub super_class: Option<TirExpr>,
    /// Static fields / consts: name + optional initializer.
    pub statics: Vec<(Arc<str>, Option<TirExpr>)>,
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
    pub key: Arc<str>,
    pub func: FnId,
    pub is_static: bool,
    pub is_private: bool,
    /// Method decorators, applied innermost-first.
    pub decorators: Vec<TirExpr>,
}

#[derive(Debug, Clone)]
pub struct TirClassAccessor {
    pub key: Arc<str>,
    pub func: FnId,
    pub is_getter: bool,
    pub is_static: bool,
}

#[derive(Debug, Clone)]
pub struct TirVariantDef {
    pub name: Arc<str>,
    pub tag: i64,
    pub meta: Arc<str>,
    pub const_args: Vec<TirExpr>,
}

#[derive(Debug, Clone)]
pub enum TirImportKind {
    Default,
    Named(Arc<str>),
    Namespace,
}

#[derive(Debug, Clone)]
pub struct TirImportSpec {
    pub local: Arc<str>,
    pub kind: TirImportKind,
}

/// One `import ... from "src"` — the linkage the backend turns into a
/// `LoadModule` plus a `StoreGlobal` per bound name.
#[derive(Debug, Clone)]
pub struct TirImport {
    pub source: Arc<str>,
    pub is_type_only: bool,
    pub specs: Vec<TirImportSpec>,
}

/// One name this module exposes. The backend fills the module slot named by
/// `exported` from either a local global or, for `export {..} from "src"`, a
/// property of that source module.
#[derive(Debug, Clone)]
pub struct TirExport {
    pub exported: Arc<str>,
    pub local: Arc<str>,
    /// `Some(src)` — a re-export; the value is `src`'s `local` property.
    pub reexport_from: Option<Arc<str>>,
    /// `export * as ns from "src"` — bind the whole module object.
    pub namespace: bool,
}

#[derive(Debug)]
pub struct TirModule {
    pub source_file: Arc<str>,
    pub imports: Vec<TirImport>,
    pub exports: Vec<TirExport>,
    pub types: TyTable,
    pub classes: Vec<crate::tables::ClassInfo>,
    pub enums: Vec<crate::tables::EnumInfo>,
    pub signatures: Vec<crate::tables::Signature>,
    pub functions: Vec<TirFunction>,
    pub globals: Vec<BackendTy>,
    /// The name of each global, parallel to `globals`. A `GlobalSlot(n)`
    /// resolution names `globals[n]` / `global_names[n]`.
    pub global_names: Vec<Arc<str>>,
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
