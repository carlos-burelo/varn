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

    Import,

    Reexport,

    MergeExports,

    InvokeRuntimeStatic,

    // Specialized int-immediate opcodes: [opcode|dest] [src|imm8_signed]
    // imm8 is treated as i8 (sign-extended). Used for x+k, x-k patterns.
    AddImm,
    SubImm,

    // Template string builder: [BuildStr|dest] [count|0] [reg0|0] [reg1|0] ...
    // All count parts (pre-converted to strings) are concatenated in one heap alloc.
    BuildStr,

    // Single-word load for the three most common integer constants.
    LoadIntZero,
    LoadIntOne,
    LoadIntMinusOne,

    // Typed integer operations
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

    // Typed float operations
    AddFloat,
    SubFloat,
    MulFloat,
    DivFloat,
    LtFloat,
    GtFloat,
    LteFloat,
    GteFloat,
    EqFloat,
    NeqFloat,

    Nop,
}

impl OpCode {
    #[inline(always)]
    pub fn from_u8(v: u8) -> Option<Self> {
        if v <= OpCode::Nop as u8 {
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
