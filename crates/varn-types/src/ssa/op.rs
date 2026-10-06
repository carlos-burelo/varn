use serde::{Deserialize, Serialize};

use super::operators::{SsaBinOp, SsaUnOp};

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

    SelfCall {
        args: Vec<u32>,
    },

    Call {
        callee: u32,
        args: Vec<u32>,
    },

    AllocInstance {
        class: u32,
    },

    LoadGlobalIdx(u32),

    ArrayGetIndex {
        object: u32,
        index: u32,
    },

    ArraySetIndex {
        object: u32,
        index: u32,
        value: u32,
    },

    ConstChar(char),

    ConstBigInt(Box<str>),

    ConstDecimal(Box<str>),

    MakeEnumVariant {
        tag: i64,
        meta: Box<str>,
    },

    IntrinsicCall {
        object: u32,
        args: Vec<u32>,
        wire: u8,
    },

    LoadNativeGlobalIdx(u32),

    Try {
        catch_ip: u32,
        catch_value: u32,
        live: Vec<u32>,
    },

    PopTry,

    CatchParam {
        try_val: u32,
    },

    StoreGlobalIdx {
        slot: u32,
        value: u32,
    },

    MakeClosure {
        proto: u32,
        upvalues: Vec<SsaUpvalue>,
    },

    LoadCaptured {
        var: u32,
    },

    StoreCaptured {
        var: u32,
        value: u32,
    },

    LoadUpvalue(u32),

    StoreUpvalue {
        index: u32,
        value: u32,
    },

    CloseUpvalues {
        vars: Vec<u32>,
    },

    Cast {
        operand: u32,
    },

    Convert {
        operand: u32,
        conv: varn_core::NumConv,
    },

    IsNull {
        operand: u32,
    },

    Typeof {
        operand: u32,
    },

    ToString {
        operand: u32,
    },

    IsArray {
        operand: u32,
    },

    GetEnumTag {
        operand: u32,
    },

    ObjectKeys {
        operand: u32,
    },

    BuildStr {
        parts: Vec<u32>,
    },

    BuildArray {
        elements: Vec<u32>,
    },

    BuildMap {
        pairs: Vec<(u32, u32)>,
    },

    BuildObject {
        keys: Vec<Box<str>>,
        values: Vec<u32>,
        is_record: bool,
    },

    GetProperty {
        object: u32,
        name: Box<str>,
        cs: u16,
    },

    SetProperty {
        object: u32,
        value: u32,
        name: Box<str>,
        cs: u16,
    },

    GetIndex {
        object: u32,
        index: u32,
    },

    SetIndex {
        object: u32,
        index: u32,
        value: u32,
    },

    ArrayLength {
        operand: u32,
    },

    StrLength {
        operand: u32,
    },

    BytesLength {
        operand: u32,
    },

    MethodCall {
        recv: u32,
        name: Box<str>,
        args: Vec<u32>,
        cs: u16,
    },

    CallNativeOp {
        object: u32,
        args: Vec<u32>,
        op_id: u64,
    },

    ArrayPush {
        array: u32,
        value: u32,
    },

    This,

    GetFixedField {
        object: u32,
        slot: u16,
        offset: u32,
        access: varn_core::FieldAccess,
    },

    SetFixedField {
        object: u32,
        value: u32,
        slot: u16,
        offset: u32,
        kind: Option<varn_core::RuntimeKind>,
    },

    MakeClass {
        name: Box<str>,
        super_class: Option<u32>,
    },

    DeclareLayout {
        class: u32,
        layout: std::sync::Arc<varn_core::layout::ClassLayout>,
    },

    DefineMethod {
        class: u32,
        name: Box<str>,
        member: u32,
        kind: u8,
    },

    GetSuper {
        name: Box<str>,
    },

    LoadGlobal(Box<str>),

    StoreGlobal {
        name: Box<str>,
        value: u32,
    },

    BuildTuple {
        elements: Vec<u32>,
    },

    BuildArraySpread {
        elements: Vec<SsaSpread>,
    },

    BuildObjectSpread {
        parts: Vec<SsaObjectSpreadPart>,
        cs_base: u16,
    },

    ObjectMerge {
        target: u32,
        source: u32,
    },

    ObjectRest {
        object: u32,
        skip_keys: Vec<Box<str>>,
    },

    GetPropertyMaybe {
        object: u32,
        name: Box<str>,
    },

    AssertNotNull {
        operand: u32,
    },

    BindMethod {
        object: u32,
        name: Box<str>,
    },

    ArrayExtend {
        array: u32,
        source: u32,
    },

    WrapSpread {
        operand: u32,
    },

    Range {
        start: u32,
        end: u32,
        inclusive: bool,
    },

    GetSymbol {
        object: u32,
        is_async: bool,
    },

    IterCall {
        callee: u32,
        recv: u32,
    },

    SuperCall {
        args: Vec<u32>,
    },

    SuperMethodCall {
        name: Box<str>,
        args: Vec<u32>,
    },

    ExtensionCall {
        func: Box<str>,
        slot: Option<u32>,
        recv: u32,
        args: Vec<u32>,
    },

    CallSpread {
        callee: u32,
        args: Vec<SsaSpread>,
    },

    LoadModule {
        source: Box<str>,
        own_ip: u32,
        live: Vec<u32>,
    },

    ModuleSlot {
        object: u32,
        slot: u16,
    },

    StoreModuleSlot {
        slot: u16,
        value: u32,
    },

    Await {
        operand: u32,
        resume_ip: u32,
        live: Vec<u32>,
    },

    Spawn {
        operand: u32,
    },

    Yield {
        operand: u32,
        resume_ip: u32,
        live: Vec<u32>,
    },

    Dispose {
        var: u32,
        is_await: bool,
        cs: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsaSpread {
    pub value: u32,
    pub spread: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsaObjectSpreadPart {
    pub key: Option<Box<str>>,
    pub value: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SsaUpvalue {
    Captured(u32),

    Inherited(u32),
}

pub const UPVALUE_LOCAL: u64 = 1 << 32;
