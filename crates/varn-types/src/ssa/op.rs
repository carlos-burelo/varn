//! The typed operations of [`super::SsaInst`], and the upvalue sources a
//! closure op names.

use serde::{Deserialize, Serialize};
use varn_core::RuntimeKind;

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

    /// Cross-function call. `callee` is a `Ref`/`Dyn` value (a closure) and
    /// `callee_global` is the module-relative global slot it was loaded from,
    /// when known — what the linker resolves a static target by. The fallback
    /// is always the canonical `ExecCtx::invoke`.
    Call {
        callee: u32,
        callee_global: Option<u32>,
        args: Vec<u32>,
    },

    /// Module-relative global read (`GlobalStore[closure.module_base + slot]`),
    /// a boxed `VmValue` (`Ref`/`Dyn`).
    LoadGlobalIdx(u32),

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

    /// `DeclareField` on a class; no result.
    DeclareField {
        class: u32,
        name: Box<str>,
        tag: Option<RuntimeKind>,
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
