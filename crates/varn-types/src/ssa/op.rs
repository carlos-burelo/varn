//! The typed operations of [`super::SsaInst`], and the upvalue sources a
//! closure op names.

use serde::{Deserialize, Serialize};

use super::operators::{SsaBinOp, SsaUnOp};

/// Typed operation of the scalar/arith family.
///
/// The scalar arithmetic and comparison variants encode the width the checker
/// proved (`IntAdd` vs `FloatAdd`), so a backend never inspects operand types
/// to choose an instruction. `Cast` is a representation-neutral fact the
/// checker emitted; `Convert` is the one op that changes a numeric domain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SsaOp {
    ConstInt(i64),
    ConstFloat(f64),
    ConstBool(bool),
    ConstNull,
    ConstStr(Box<str>),

    Binary {
        op: SsaBinOp,
        lhs: u32,
        rhs: u32,
    },

    Unary {
        op: SsaUnOp,
        operand: u32,
    },

    /// Direct self-recursion. The callee is this same proto, so no linker or
    /// heap reference is involved — only the call arguments, in order.
    SelfCall {
        args: Vec<u32>,
    },

    /// Cross-function call. `callee` is a `Ref`/`Dyn` value (a closure); the
    /// fallback is always the canonical `ExecCtx::invoke`.
    Call {
        callee: u32,
        args: Vec<u32>,
    },

    /// A fresh instance of `class`, its constructor not yet applied.
    AllocInstance {
        class: u32,
    },

    /// Module-relative global read (`GlobalStore[closure.module_base + slot]`),
    /// a boxed `VmValue` (`Ref`/`Dyn`).
    LoadGlobalIdx(u32),

    /// `object[index]` with `object` proven an array and `index` an `int`:
    /// the element, read inline in the representation of the destination.
    ArrayGetIndex {
        object: u32,
        index: u32,
    },

    /// `object[index] = value` on a proven array and `int` index; no result.
    ArraySetIndex {
        object: u32,
        index: u32,
        value: u32,
    },

    /// A `char` literal of this proto's constant pool.
    ConstChar(char),

    /// A `bigint` literal of the pool, by its canonical base-10 digits — a
    /// heap result.
    ConstBigInt(Box<str>),

    /// A `decimal` literal of the pool, by its canonical text — a heap
    /// result.
    ConstDecimal(Box<str>),

    /// The enum variant with discriminant `tag` described by `meta`
    /// (`Enum.Variant[:field,...]`) — a heap result.
    MakeEnumVariant {
        tag: i64,
        meta: Box<str>,
    },

    /// A `std:math` intrinsic, selected by `wire`, on `[object, args...]` —
    /// `object` is the free function's null receiver. A boxed result.
    IntrinsicCall {
        object: u32,
        args: Vec<u32>,
        wire: u8,
    },

    /// Prelude / host global read at its absolute native-layout index, a
    /// boxed `VmValue`.
    LoadNativeGlobalIdx(u32),

    /// Open a `try` region. A throw inside it resumes the INTERPRETER at the
    /// landing pad (`catch_ip`, a bytecode offset) with the thrown value in
    /// `catch_value`'s register; the catch path never runs compiled. `live`
    /// are the values the landing pad reads, which must be in their homes
    /// while the region is open.
    Try {
        catch_ip: u32,
        catch_value: u32,
        live: Vec<u32>,
    },

    /// Close the innermost `try` region; no result.
    PopTry,

    /// The thrown value a landing pad starts from. Only a landing pad holds
    /// it, and landing pads run interpreted.
    CatchParam {
        try_val: u32,
    },

    /// Module-relative global write of `value`; no result.
    StoreGlobalIdx {
        slot: u32,
        value: u32,
    },

    /// A closure over function constant `proto` of this proto's pool,
    /// capturing `upvalues` in order — a `Ref` result. With no upvalues it is
    /// the function's one shared closure.
    MakeClosure {
        proto: u32,
        upvalues: Vec<SsaUpvalue>,
    },

    /// Read captured variable `var` (an index into [`SsaProto::captured`]).
    /// A captured variable is not an SSA value: it lives in its frame
    /// register for its whole life, which the closures capturing it share.
    LoadCaptured {
        var: u32,
    },

    /// Write captured variable `var`; no result.
    StoreCaptured {
        var: u32,
        value: u32,
    },

    /// This closure's upvalue `index` — a boxed result.
    LoadUpvalue(u32),

    /// Write this closure's upvalue `index`; no result.
    StoreUpvalue {
        index: u32,
        value: u32,
    },

    /// Close the open upvalues over captured variables `vars` (and every
    /// register above the lowest of them): a loop body's per-iteration
    /// bindings, so the next iteration's closures capture fresh ones.
    CloseUpvalues {
        vars: Vec<u32>,
    },

    /// Checker-proven, representation-neutral cast.
    Cast {
        operand: u32,
    },
    /// Numeric conversion (`as`) that changes representation.
    Convert {
        operand: u32,
        conv: varn_core::NumConv,
    },

    IsNull {
        operand: u32,
    },

    /// `typeof x` — a heap string result.
    Typeof {
        operand: u32,
    },

    /// `String(x)` — a heap string result.
    ToString {
        operand: u32,
    },

    /// Runtime array test; a `bool` result.
    IsArray {
        operand: u32,
    },

    /// Enum discriminant of `operand`; an `int` result.
    GetEnumTag {
        operand: u32,
    },

    /// Own enumerable keys of `operand`; a heap array result.
    ObjectKeys {
        operand: u32,
    },

    /// Interpolated string from `parts`; a heap string result.
    BuildStr {
        parts: Vec<u32>,
    },

    /// Array literal from `elements`; a heap array result.
    BuildArray {
        elements: Vec<u32>,
    },

    /// Map literal from `pairs`; a heap map result.
    BuildMap {
        pairs: Vec<(u32, u32)>,
    },

    /// Object/record literal from `keys`/`values`; a heap result. The shape is
    /// resolved from the proto's pool by key match at lowering time.
    BuildObject {
        keys: Vec<Box<str>>,
        values: Vec<u32>,
        is_record: bool,
    },

    /// Dynamic property read; `cs` is the inline-cache slot. A heap result.
    GetProperty {
        object: u32,
        name: Box<str>,
        cs: u16,
    },

    /// Dynamic property write; no result.
    SetProperty {
        object: u32,
        value: u32,
        name: Box<str>,
        cs: u16,
    },

    /// `obj[index]` — a heap result.
    GetIndex {
        object: u32,
        index: u32,
    },

    /// `obj[index] = value` — no result.
    SetIndex {
        object: u32,
        index: u32,
        value: u32,
    },

    /// `arr.length` — an `int` result.
    ArrayLength {
        operand: u32,
    },

    /// `s.length` of a `str` — an `int` result (no allocation).
    StrLength {
        operand: u32,
    },

    /// `b.length` of `Bytes` — an `int` result (no allocation).
    BytesLength {
        operand: u32,
    },

    /// `recv.name(args)`: a method call through the runtime's one method
    /// resolution (inline cache slot `cs`); a boxed result.
    MethodCall {
        recv: u32,
        name: Box<str>,
        args: Vec<u32>,
        cs: u16,
    },

    /// A core-type method the checker resolved to a native op: `op_id`
    /// called with the receiver `object` then `args`; a boxed result.
    CallNativeOp {
        object: u32,
        args: Vec<u32>,
        op_id: u64,
    },

    /// `arr.push(value)` — no result.
    ArrayPush {
        array: u32,
        value: u32,
    },

    /// The current receiver (`this`), read from home 0; a heap result.
    This,

    /// Fixed-field read. A class field (`access: Compact`) is at the payload
    /// `offset` the compiler laid out, in its kind's representation; an
    /// object/record/enum-payload field (`Slot`) is found by `slot`, which a
    /// compact access also keeps for its fallback.
    GetFixedField {
        object: u32,
        slot: u16,
        offset: u32,
        access: varn_core::FieldAccess,
    },

    /// Class field write at the payload `offset`, laid out by `kind`; no
    /// result.
    SetFixedField {
        object: u32,
        value: u32,
        slot: u16,
        offset: u32,
        kind: Option<varn_core::RuntimeKind>,
    },

    /// `class Name [extends Super]` — a heap class object.
    MakeClass {
        name: Box<str>,
        super_class: Option<u32>,
    },

    /// The complete instance layout of a class; no result.
    DeclareLayout {
        class: u32,
        layout: std::sync::Arc<varn_core::layout::ClassLayout>,
    },

    /// A class member definition (`Method`/`DefineStatic`/accessors). `kind` is
    /// the runtime discriminant: Method=0, DefineStatic=1, DefineGetter=2,
    /// DefineSetter=3, DefineStaticGetter=4, DefineStaticSetter=5.
    DefineMethod {
        class: u32,
        name: Box<str>,
        member: u32,
        kind: u8,
    },

    /// `GetSuper name` — a heap result.
    GetSuper {
        name: Box<str>,
    },

    /// Name-keyed global read (unresolved at compile time) — a heap result.
    /// Lowered through a runtime helper; the indexed forms above stay fast.
    LoadGlobal(Box<str>),

    /// Name-keyed global write; no result.
    StoreGlobal {
        name: Box<str>,
        value: u32,
    },

    /// Tuple literal from `elements`; a heap array result (same layout as
    /// `BuildArray`, distinct opcode in bytecode).
    BuildTuple {
        elements: Vec<u32>,
    },

    /// Array literal with spreads: `elements` in order, `spread[i]` telling
    /// whether `elements[i]` spreads (`ArrayExtend`) or pushes (`ArrayPush`).
    BuildArraySpread {
        elements: Vec<SsaSpread>,
    },

    /// Object literal with spreads: `Some(key)` sets a property, `None`
    /// merges (`ObjectMerge`). `cs_base` is the first inline-cache slot the
    /// compiler numbered for the keyed parts (consecutive, one per `Some`).
    BuildObjectSpread {
        parts: Vec<SsaObjectSpreadPart>,
        cs_base: u16,
    },

    /// `ObjectMerge target source` (`{...a, ...b}` tail); no result.
    ObjectMerge {
        target: u32,
        source: u32,
    },

    /// `ObjectRest object skip_keys` (`const {a, ...rest}`); heap result.
    ObjectRest {
        object: u32,
        skip_keys: Vec<Box<str>>,
    },

    /// `obj?.name` — a heap result.
    GetPropertyMaybe {
        object: u32,
        name: Box<str>,
    },

    /// `x!` non-null assertion; no result (traps on null).
    AssertNotNull {
        operand: u32,
    },

    /// Bound method (`obj::method`); a heap closure result.
    BindMethod {
        object: u32,
        name: Box<str>,
    },

    /// `arr.extend(src)`; no result.
    ArrayExtend {
        array: u32,
        source: u32,
    },

    /// Spread marker for a call element; a heap wrapper result.
    WrapSpread {
        operand: u32,
    },

    /// `start..end` / `start..=end`; a heap range result.
    Range {
        start: u32,
        end: u32,
        inclusive: bool,
    },

    /// Iterator/async-iterator symbol of `object`; a heap result.
    GetSymbol {
        object: u32,
        is_async: bool,
    },

    /// Iterator protocol call `callee.recv`; a heap result.
    IterCall {
        callee: u32,
        recv: u32,
    },

    /// `super(...args)` constructor call; a heap result.
    SuperCall {
        args: Vec<u32>,
    },

    /// `super.name(...args)`; a heap result.
    SuperMethodCall {
        name: Box<str>,
        args: Vec<u32>,
    },

    /// Extension-function call `func(recv, ...args)`; a heap result.
    /// `slot` is the callee's module-global slot when numbered.
    ExtensionCall {
        func: Box<str>,
        slot: Option<u32>,
        recv: u32,
        args: Vec<u32>,
    },

    /// Spread call `callee(...args)`; a heap result.
    CallSpread {
        callee: u32,
        args: Vec<SsaSpread>,
    },

    /// `import source`; a heap module-namespace result (may suspend).
    /// `own_ip` is the bytecode offset of the emitting instruction: a suspend
    /// rewinds the frame to re-execute the load once the import resolves.
    /// `live` is what the interpreter reads from the homes when it resumes.
    LoadModule {
        source: Box<str>,
        own_ip: u32,
        live: Vec<u32>,
    },

    /// `module[slot]` namespace read; a heap result.
    ModuleSlot {
        object: u32,
        slot: u16,
    },

    /// Namespace write; no result.
    StoreModuleSlot {
        slot: u16,
        value: u32,
    },

    /// `await operand`; its value (may suspend, resumes interpreted at
    /// `resume_ip`, the bytecode offset of the next instruction).
    /// `live` is what the interpreter reads from the homes when it resumes.
    Await {
        operand: u32,
        resume_ip: u32,
        live: Vec<u32>,
    },

    /// `spawn operand`; a heap task handle (never suspends the caller).
    Spawn {
        operand: u32,
    },

    /// `yield operand`; its value (suspends, resumes interpreted at
    /// `resume_ip`, the bytecode offset of the next instruction).
    /// `live` is what the interpreter reads from the homes when it resumes.
    Yield {
        operand: u32,
        resume_ip: u32,
        live: Vec<u32>,
    },

    /// `using`/`await using` disposal of captured variable `var`; no result.
    /// `cs` is the `CallMethod` cache slot the compiler numbered for the
    /// `dispose`/`disposeAsync` call.
    Dispose {
        var: u32,
        is_await: bool,
        cs: u16,
    },
}

/// One call/array element that may spread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsaSpread {
    pub value: u32,
    pub spread: bool,
}

/// One object-spread part: `Some(key)` sets, `None` merges.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsaObjectSpreadPart {
    pub key: Option<Box<str>>,
    pub value: u32,
}

/// Where a closure's upvalue comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SsaUpvalue {
    /// The creating function's captured variable (an index into
    /// [`SsaProto::captured`]).
    Captured(u32),
    /// The creating closure's own upvalue.
    Inherited(u32),
}

/// How compiled code hands a closure's upvalue sources to the runtime: one
/// word each, a frame register with this bit set, or the creating closure's
/// upvalue index without it.
pub const UPVALUE_LOCAL: u64 = 1 << 32;
