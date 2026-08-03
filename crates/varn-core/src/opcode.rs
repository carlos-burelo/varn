#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum OpCode {
    LoadConst,
    LoadNull,
    LoadTrue,
    LoadFalse,
    LoadInt,
    Move,
    LoadGlobal,
    StoreGlobal,
    DefineGlobal,
    DefineGlobalIdx,
    LoadGlobalIdx,
    StoreGlobalIdx,
    LoadUpvalue,
    StoreUpvalue,
    CloseUpvalue,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Negate,
    Not,
    ToString,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Ushr,
    Jump,
    JumpIfFalse,
    JumpIfTrue,
    Loop,
    Call,
    CallMethod,
    InvokeVirtual,
    CallSpread,
    Return,
    BuildArray,
    BuildObject,
    BuildObjectWithShape,
    GetIndex,
    SetIndex,
    ObjectRest,
    ObjectKeys,
    ObjectMerge,
    GetProperty,
    GetPropertyMaybe,
    SetProperty,
    GetFixedField,
    SetFixedField,
    GetSuper,
    GetSymbol,

    MakeClosure,

    MakeClass,

    Inherit,

    Method,

    DefineStatic,

    DefineGetter,

    DefineSetter,

    DefineStaticGetter,

    DefineStaticSetter,

    DeclareField,

    BindMethod,

    Typeof,

    Instanceof,

    In,

    IsNull,

    IsArray,

    AssertNotNull,

    StrConcat,

    StrLength,

    StrSlice,

    ArrayLength,

    ArrayPush,

    ArrayPop,

    ArrayExtend,

    WrapSpread,

    MakeEnumVariant,

    GetEnumTag,

    Await,

    Spawn,

    Yield,

    Try,

    Throw,

    PopTry,

    LoadModule,

    LoadModuleSlot,

    StoreModuleSlot,

    InvokeRuntimeStatic,

    AddImm,
    SubImm,

    BuildStr,

    LoadIntZero,
    LoadIntOne,
    LoadIntMinusOne,

    AddInt,
    SubInt,
    MulInt,
    DivInt,
    LtInt,
    GtInt,
    LteInt,
    GteInt,
    EqInt,
    NeqInt,

    AddFloat,
    SubFloat,
    MulFloat,
    DivFloat,
    ModFloat,
    PowFloat,
    LtFloat,
    GtFloat,
    LteFloat,
    GteFloat,
    EqFloat,
    NeqFloat,

    ModInt,
    PowInt,

    Intrinsic,

    LoadStaticFn,

    CallSelf,

    Nop,

    ArrayGetIndex,
    ArraySetIndex,

    /// Direct dispatch of a statically-typed core-type method by stable op-id.
    /// Operands: `[op_id_const_idx: u16][arg_count: u16]` where `arg_count`
    /// includes the receiver. The receiver sits at the packed `call_base`
    /// register, args contiguous above it; the result is written back to
    /// `call_base`. Bypasses the string method name + inline-cache lookup.
    CallNativeOp,

    /// A unary `std:math` intrinsic with direct operands, bypassing the call
    /// window `Intrinsic` needs. Operands: `[src: u8][wire_byte: u8]` packed
    /// into one word, result into the packed `dest` register.
    ///
    /// `Intrinsic` writes its result to the SAME register that stages the
    /// call receiver, so that register can never be float-typed — every
    /// argument gets boxed on the way in and the result unboxed on the way
    /// out, around ops that are a single IEEE instruction. This form has no
    /// receiver and no window, so both ends stay in their natural
    /// representation. Only unary math ops qualify: every other domain
    /// actually reads its receiver.
    IntrinsicDirect,
}

impl OpCode {
    #[inline(always)]
    pub fn from_u8(v: u8) -> Option<Self> {
        if v <= OpCode::IntrinsicDirect as u8 {
            Some(unsafe { std::mem::transmute::<u8, OpCode>(v) })
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn from_u16(v: u16) -> Option<Self> {
        Self::from_u8(v as u8)
    }
}
