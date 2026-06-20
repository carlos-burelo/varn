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
use rust_decimal::Decimal;

pub mod dump;
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
    /// `>>>` unsigned right shift.
    Ushr,
    /// `x instanceof C` — class membership test.
    Instanceof,
    /// `k in obj` — property presence test.
    In,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirUnOp {
    Neg,
    Not,
    BitNot,
    Typeof,
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

/// A resolved `expr is Type` test. The AST `TypeNode` is reduced at lowering to
/// one concrete runtime check, mirroring legacy `member::compile_is`.
#[derive(Debug, Clone)]
pub enum HirTypeTest {
    /// `IsNull`.
    IsNull,
    /// `IsArray`.
    IsArray,
    /// `typeof value === <intrinsic name>` (scalar primitives).
    TypeofEq(Rc<str>),
    /// `value instanceof <global class>` (looked up by name).
    Instanceof(Rc<str>),
    /// Unsupported type form → constant `false` (`LoadFalse`).
    AlwaysFalse,
}

#[derive(Debug, Clone)]
pub enum HirExpr {
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Bool(bool),
    Char(char),
    Decimal(Decimal),
    BigInt(i128),
    Regex { pattern: Rc<str>, flags: Rc<str> },
    Null,
    /// `expr!` non-null assertion → `AssertNotNull` (value passes through).
    NonNull(Box<HirExpr>),
    /// `expr?` try operator: if the operand is a non-ok enum (Result.Err /
    /// Option.None) early-return it from the enclosing function, else yield it.
    /// Mirrors legacy `compile_try_expr` (`GetEnumTag` + `JumpIfTrue` + `Return`).
    TryOp(Box<HirExpr>),
    /// `expr is Type` runtime type test, resolved at lowering to a concrete
    /// check (`IsNull`/`IsArray`/`Typeof`-compare/`Instanceof`/constant false).
    TypeTest {
        value: Box<HirExpr>,
        kind: HirTypeTest,
    },
    /// Comma expression: evaluate all, yield the last.
    Sequence(Vec<HirExpr>),
    /// `start..end` / `start..=end` → runtime range object.
    Range {
        start: Box<HirExpr>,
        end: Box<HirExpr>,
        inclusive: bool,
    },
    /// Template literal `` `a${x}b` `` → `BuildStr` over literal/interpolated parts.
    Template(Vec<HirTemplatePart>),
    /// Assignment used as an expression (yields the assigned value).
    Assign {
        target: Box<HirAssignTarget>,
        value: Box<HirExpr>,
    },
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
        target: Box<HirAssignTarget>,
        op: HirUpdateOp,
        prefix: bool,
    },
    /// Array literal supporting spreads and holes.
    Array(Vec<HirArrayEl>),
    /// `object?.name` maybe member access / GetPropertyMaybe.
    MemberMaybe {
        object: Box<HirExpr>,
        name: Rc<str>,
        ty: HirType,
    },
    /// `ObjectRest` for object destructuring: `{ ...rest } = obj`.
    ObjectRest {
        object: Box<HirExpr>,
        skip_keys: Vec<Rc<str>>,
    },
    /// `object?.name` / `object?.[index]` optional chaining.
    OptionalChain {
        object: Box<HirExpr>,
        property: HirOptionalProperty,
    },
    /// Object literal with a fixed shape: all-static keys, plain value props
    /// (no computed keys, methods, getters/setters, or spreads).
    Object {
        properties: Vec<HirObjectProp>,
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
    /// Standalone super expression.
    Super,
    /// `super(args)` — superclass constructor call (`GetSuper "constructor"` +
    /// call with `this` as receiver).
    SuperCall { args: Vec<HirExpr> },
    /// `super.name(args)` — superclass method call (`GetSuper name` + call).
    SuperMethodCall { name: Rc<str>, args: Vec<HirExpr> },
    /// `super.name` — superclass member read (`GetSuper name`).
    SuperMember { name: Rc<str> },
    /// An extension call `recv.m(args)` / getter `recv.k` / setter `recv.k = v`,
    /// lowered to the mangled global `func` invoked with `recv` as the **receiver**
    /// (`this`, register 0 of the callee), args following. Distinct from `Call`
    /// because the receiver occupies the receiver slot, not an argument slot.
    ExtensionCall {
        func: Rc<str>,
        recv: Box<HirExpr>,
        args: Vec<HirExpr>,
    },
    /// A class value: `MakeClass` + `DeclareField`(s) + `Method`(s). Bound to a
    /// global or local by the caller. Core subset: no inheritance/static/
    /// accessors/decorators.
    Class(Box<HirClass>),
    /// An enum value: `MakeClass` + per-variant `MakeEnumVariant`/`DefineStatic`
    /// + instance fields/methods.
    Enum(Box<HirEnum>),
    /// `match subject { cases }` lowered to a branch chain (legacy
    /// `compile_match`).
    Match {
        subject: Box<HirExpr>,
        cases: Vec<HirMatchCase>,
    },
    /// A spread argument: `...expr`.
    Spread(Box<HirExpr>),
    /// `await expr`
    Await(Box<HirExpr>),
    /// `spawn expr`
    Spawn(Box<HirExpr>),
    /// `yield expr`
    Yield(Box<HirExpr>),
    /// Tagged template: `tag`template``
    TaggedTemplate {
        tag: Box<HirExpr>,
        template: Box<HirExpr>,
    },
    /// Intrinsic call on a member: `obj.intrinsic(args)`
    IntrinsicCall {
        object: Box<HirExpr>,
        args: Vec<HirExpr>,
        wire_byte: u8,
        ty: HirType,
    },
    /// Module slot read: `obj` is the module object
    ModuleSlot {
        object: Box<HirExpr>,
        slot: u16,
        ty: HirType,
    },
}

#[derive(Debug, Clone)]
pub enum HirArrayEl {
    Expr(HirExpr),
    Spread(HirExpr),
    Hole,
}

#[derive(Debug, Clone)]
pub enum HirObjectProp {
    Property {
        key: HirPropKey,
        value: HirExpr,
    },
    Method {
        key: HirPropKey,
        func: HirFunction,
        upvalues: Vec<HirUpvalueSrc>,
    },
    Spread(HirExpr),
}

#[derive(Debug, Clone)]
pub enum HirPropKey {
    Static(Rc<str>),
    Computed(HirExpr),
}

#[derive(Debug, Clone)]
pub enum HirOptionalProperty {
    Member(Rc<str>),
    Index(Box<HirExpr>),
    ModuleSlot(u16),
    Extension(Rc<str>),
    Call(Vec<HirExpr>),
    MethodCall(Rc<str>, Vec<HirExpr>),
    ExtensionCall(Rc<str>, Vec<HirExpr>),
}

/// One piece of a template literal.
#[derive(Debug, Clone)]
pub enum HirTemplatePart {
    Str(Rc<str>),
    Expr(HirExpr),
}

/// The target of an assignment-expression.
#[derive(Debug, Clone)]
pub enum HirAssignTarget {
    /// `name = value`.
    Var(HirBinding),
    /// `object.name = value` → `SetProperty`.
    Member { object: HirExpr, name: Rc<str> },
    /// `object[index] = value` → `SetIndex`.
    Index { object: HirExpr, index: HirExpr },
    /// `ModuleSlot = value` → `StoreModuleSlot`.
    ModuleSlot { slot: u16 },
    /// `super.name = value`.
    SuperMember { name: Rc<str> },
    /// `super[index] = value`.
    SuperIndex { index: HirExpr },
}

/// An enum variant: a tag, a metadata string (`Enum.Variant[:fields]`), and
/// whether it carries a payload.
#[derive(Debug, Clone)]
pub struct HirEnumVariant {
    pub name: Rc<str>,
    pub tag: i64,
    pub meta: Rc<str>,
    pub const_args: Vec<HirExpr>,
}

#[derive(Debug, Clone)]
pub struct HirEnum {
    pub name: Rc<str>,
    pub variants: Vec<HirEnumVariant>,
    pub fields: Vec<Rc<str>>,
    pub static_fields: Vec<(Rc<str>, Option<HirExpr>)>,
    pub ctor: HirMethod,
    pub methods: Vec<HirMethod>,
    pub static_methods: Vec<HirMethod>,
    pub getters: Vec<HirAccessor>,
    pub setters: Vec<HirAccessor>,
    pub static_blocks: Vec<HirMethod>,
}

#[derive(Debug, Clone)]
pub struct HirMatchCase {
    pub test: HirCaseTest,
    /// Optional `if`-guard: the arm matches only when the pattern matches **and**
    /// the guard is truthy (else falls through to the next case). Lowered in the
    /// case's scope so it sees the pattern bindings.
    pub guard: Option<HirExpr>,
    pub body: Vec<HirStmt>,
    /// The arm's value (expression body, or `None` for a block arm → null).
    pub result: Option<HirExpr>,
}

#[derive(Debug, Clone)]
pub enum HirCaseTest {
    /// `_` — always matches, no binding.
    Wildcard,
    /// `subject == literal`.
    Literal(HirExpr),
    /// Bind the subject to a local; always matches.
    Bind(LocalId),
    /// Match an enum variant by name, binding its `value{i}` payloads.
    EnumVariant {
        name: Rc<str>,
        binds: Vec<Option<LocalId>>,
    },
    /// Match a record pattern, binding properties.
    Record {
        fields: Vec<(Rc<str>, Option<LocalId>)>,
    },
}

/// A class method (or constructor) plus the upvalues its closure captures.
#[derive(Debug, Clone)]
pub struct HirMethod {
    pub key: Rc<str>,
    pub func: HirFunction,
    pub upvalues: Vec<HirUpvalueSrc>,
    pub decorators: Vec<HirExpr>,
    pub is_private: bool,
}

/// A class accessor (getter or setter), instance or static. Lowers to
/// `DefineGetter`/`DefineSetter` (or the `DefineStatic*` variants).
#[derive(Debug, Clone)]
pub struct HirAccessor {
    pub key: Rc<str>,
    pub func: HirFunction,
    pub upvalues: Vec<HirUpvalueSrc>,
    pub is_static: bool,
}

#[derive(Debug, Clone)]
pub struct HirClass {
    pub name: Rc<str>,
    /// Superclass value (`extends`), lowered as an expression; `MakeClass`
    /// takes its register so the VM links the prototype chain. `None` = no base.
    pub super_class: Option<HirExpr>,
    /// Instance field names (`DeclareField`); initializers live in `ctor`.
    pub fields: Vec<Rc<str>>,
    /// Static fields (`DefineStatic`): the initializer is evaluated at class
    /// build time (or `null` when absent).
    pub static_fields: Vec<(Rc<str>, Option<HirExpr>)>,
    /// The constructor (synthesised if the source omits one). Runs field
    /// initializers after its body, matching legacy.
    pub ctor: HirMethod,
    /// Instance methods (`Method`); a `destructor` is lowered here as `dispose`.
    pub methods: Vec<HirMethod>,
    /// Static methods (`DefineStatic` with the closure as value).
    pub static_methods: Vec<HirMethod>,
    /// Getters/setters (instance + static).
    pub getters: Vec<HirAccessor>,
    pub setters: Vec<HirAccessor>,
    /// Static initializer blocks, invoked (`Call`) immediately at build time.
    pub static_blocks: Vec<HirMethod>,
    pub decorators: Vec<HirExpr>,
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
    /// Pre-tested loop (`while`).
    While {
        test: HirExpr,
        body: Vec<HirStmt>,
    },
    /// C-style `for`: `test` is checked at the top, `update` runs after the body
    /// (and is where `continue` lands, so it is not skipped). `init` is lowered
    /// as ordinary statements before this node.
    ForClassic {
        test: HirExpr,
        update: Vec<HirStmt>,
        body: Vec<HirStmt>,
    },
    /// `for (var of iterable) body` — iterator protocol (`Symbol.iterator`/
    /// `next`/`done`/`value`). `var` is bound each iteration.
    ForOf {
        var: LocalId,
        iterable: HirExpr,
        body: Vec<HirStmt>,
        is_await: bool,
    },
    /// `for (var in object) body` — `ObjectKeys` + index loop. `var` is bound to
    /// each key each iteration.
    ForIn {
        var: LocalId,
        object: HirExpr,
        body: Vec<HirStmt>,
    },
    /// `do body while (test)` — body runs once before the test.
    DoWhile {
        body: Vec<HirStmt>,
        test: HirExpr,
    },
    /// `switch (disc) { cases }` — a sequential `Eq` test chain with fall-through
    /// (legacy `stmt.rs`). All cases share one block scope.
    Switch {
        disc: HirExpr,
        cases: Vec<HirSwitchCase>,
    },
    Break,
    Continue,
    /// `throw expr` → `Throw`.
    Throw(HirExpr),
    /// `try { block } [catch] [finally]`. Lowered with `Try`/`PopTry`/`Throw`
    /// (legacy `stmt.rs`): the thrown value is bound to the optional catch local.
    Try {
        block: Vec<HirStmt>,
        catch: Option<HirCatch>,
        finally: Option<Vec<HirStmt>>,
    },
    /// Close any open upvalues that point at the given capture targets' slots,
    /// emitted when a block/function that declared captured bindings is about
    /// to go out of scope. Lowers to `CloseUpvalue` at the lowest such register.
    CloseUpvalues(Vec<CaptureTarget>),
    /// `import … from "source"` → `LoadModule` (+ per-specifier binding). A bare
    /// or type-only import has no specifiers (side-effect load only).
    Import {
        source: Rc<str>,
        is_type: bool,
        specs: Vec<HirImportSpec>,
    },
    /// Publish a module export: load the global `name` and `StoreModuleSlot`.
    StoreExport {
        name: Rc<str>,
        slot: u16,
    },
    /// Publish a named module export or re-export from a source.
    ExportNamed {
        specifiers: Vec<HirExportSpec>,
        source: Option<Rc<str>>,
    },
    /// Re-export all members from a module: `export * from "source"`.
    ExportAll {
        source: Rc<str>,
        alias: Option<Rc<str>>,
        slot: Option<u16>,
    },
    /// Default expression export: `export default expr;`.
    ExportDefaultExpr {
        value: HirExpr,
        slot: Option<u16>,
    },
    /// Dispose a `using` resource at block exit: `target.dispose()` (or
    /// `disposeAsync()`). Lowers to a no-arg `CallMethod`.
    Dispose {
        target: LocalId,
        is_await: bool,
    },
}

#[derive(Debug, Clone)]
pub struct HirExportSpec {
    pub binding: HirBinding,
    pub local: Rc<str>,
    pub exported: Rc<str>,
    pub local_slot: Option<u16>,
    pub exported_slot: Option<u16>,
}

/// A `switch` case: `test` is `None` for the `default` clause. Bodies fall
/// through to the next case (no implicit `break`), matching legacy.
#[derive(Debug, Clone)]
pub struct HirSwitchCase {
    pub test: Option<HirExpr>,
    pub body: Vec<HirStmt>,
}

/// A `catch` clause: an optional bound error local and the handler body.
#[derive(Debug, Clone)]
pub struct HirCatch {
    /// The bound error local (identifier catch param), or `None` for `catch {}`.
    pub param: Option<LocalId>,
    pub body: Vec<HirStmt>,
}

#[derive(Debug, Clone)]
pub struct HirImportSpec {
    pub local: Rc<str>,
    pub kind: HirImportKind,
    /// Pre-resolved module slot, else the value is fetched by property.
    pub slot: Option<u16>,
}

#[derive(Debug, Clone)]
pub enum HirImportKind {
    /// `import x from …` → property `default`.
    Default,
    /// `import { a as x } …` → property `a`.
    Named(Rc<str>),
    /// `import * as x …` → the module object itself.
    Namespace,
}

#[derive(Debug, Clone)]
pub struct HirParam {
    pub name: Rc<str>,
    pub ty: HirType,
    /// Default value (`fn(x = expr)`), lowered in the function's own scope. The
    /// emitter guards it with `IsNull`: if the slot is null at entry, the
    /// default is evaluated and moved in (legacy `function.rs` param prologue).
    /// Optional params (`x?`) without a default need no codegen — the VM passes
    /// null when the argument is absent.
    pub default: Option<HirExpr>,
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
    /// Whether the last parameter is a rest param (`...args`). Sets
    /// `FunctionProto.has_rest`; the VM collects surplus arguments into an array
    /// in that slot. No extra bytecode is emitted.
    pub has_rest: bool,
    pub is_async: bool,
    pub is_generator: bool,
}

/// A whole module: the synthetic top-level function plus the functions it
/// declares.
#[derive(Debug, Clone)]
pub struct HirModule {
    pub top_level: HirFunction,
    pub functions: Vec<HirFunction>,
}
