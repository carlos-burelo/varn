use rustc_hash::FxHashMap;
use varn_core::OpCode;

use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct IrModule {
    pub functions: Vec<IrFunction>,
    pub structs: Vec<IrStruct>,
    pub enums: Vec<IrEnum>,
    pub constants: Vec<varn_types::PoolEntry>,
    pub extension_members: FxHashMap<u32, Rc<str>>,
    pub extension_set_members: FxHashMap<u32, Rc<str>>,
}

#[derive(Clone, Debug)]
pub struct FullIrProgram {
    pub ast: varn_core::ast::Program,
    pub type_annotations: varn_core::TypeAnnotations,
    pub extension_calls: FxHashMap<u32, Rc<str>>,
    pub extension_members: FxHashMap<u32, Rc<str>>,
    pub extension_set_members: FxHashMap<u32, Rc<str>>,
    pub module: IrModule,
}

#[derive(Clone, Debug)]
pub struct IrFunction {
    pub name: Rc<str>,
    pub params: Vec<IrParam>,
    pub return_type: Option<Rc<str>>,
    pub is_async: bool,
    pub is_generator: bool,
    pub has_this: bool,
    pub has_rest: bool,
    pub blocks: Vec<IrBlock>,
    pub upvalues: Vec<IrUpvalueDesc>,
    pub var_slots: u16,
}

#[derive(Clone, Debug)]
pub struct IrParam {
    pub name: Rc<str>,
    pub value: IrValue,
}

#[derive(Clone, Debug)]
pub struct IrBlock {
    pub id: usize,
    pub scope_depth: usize,
    pub phis: Vec<IrPhi>,
    pub instrs: Vec<IrInstr>,
    pub terminator: IrTerminator,
}

#[derive(Clone, Debug)]
pub struct IrPhi {
    pub dest: IrValue,

    pub inputs: Vec<(usize, IrValue)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IrValue(pub usize);

#[derive(Clone, Debug)]
pub struct IrInstr {
    pub dest: Option<IrValue>,
    pub op: IrOp,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub enum IrOp {
    LoadConst(u16),
    LoadGlobal(u16),
    StoreGlobal(u16, IrValue),
    DefineGlobal(u16, IrValue),
    LoadUpvalue(u16),
    StoreUpvalue(u16, IrValue),
    LoadLocal(u16),
    StoreLocal(u16, IrValue),
    CloseUpvalueAt(u16),
    ArrayPush(IrValue, IrValue),
    ArrayExtend(IrValue, IrValue),
    IsNull(IrValue),

    Binary {
        op: varn_core::ast::operators::BinaryOp,
        left: IrValue,
        right: IrValue,
    },
    BinaryI32 {
        op: varn_core::ast::operators::BinaryOp,
        left: IrValue,
        right: IrValue,
    },
    BinaryF64 {
        op: varn_core::ast::operators::BinaryOp,
        left: IrValue,
        right: IrValue,
    },
    Unary {
        op: varn_core::ast::operators::UnaryOp,
        val: IrValue,
    },
    Logical {
        op: varn_core::ast::operators::LogicalOp,
        left: IrValue,
        right: IrValue,
    },
    LoadTrue,
    LoadFalse,
    LoadNull,
    TemplateLiteral(Vec<IrValue>),

    New {
        callee: IrValue,
        args: Vec<IrValue>,
    },
    GetProperty {
        obj: IrValue,
        key: IrPropKey,
        is_optional: bool,
    },
    SetProperty {
        obj: IrValue,
        key: IrPropKey,
        val: IrValue,
    },
    CreateArray(Vec<IrValue>),
    CreateObject(Vec<(IrPropKey, IrValue)>),
    ObjectRest(IrValue, IrValue),

    Await(IrValue),
    Spawn(IrValue),
    Yield(Option<IrValue>),
    YieldStar(IrValue),

    Class {
        name_const: u16,
        base: Option<IrValue>,
    },
    Method {
        class: IrValue,
        name_const: u16,
        func: IrValue,
    },
    DeclareField {
        class: IrValue,
        name_const: u16,
    },
    DefineGetter {
        class: IrValue,
        name_const: u16,
        func: IrValue,
    },
    DefineSetter {
        class: IrValue,
        name_const: u16,
        func: IrValue,
    },
    DefineStatic {
        class: IrValue,
        name_const: u16,
        val: IrValue,
    },
    DefineStaticGetter {
        class: IrValue,
        name_const: u16,
        func: IrValue,
    },
    DefineStaticSetter {
        class: IrValue,
        name_const: u16,
        func: IrValue,
    },

    MakeEnumVariant {
        tag: u16,
        name_const: u16,
        payload: IrValue,
    },

    GetSymbol {
        obj: IrValue,
        symbol_idx: u16,
    },

    GetVariant {
        val: IrValue,
        variant_idx: u16,
    },
    CheckVariant {
        val: IrValue,
        variant_idx: u16,
    },

    Call {
        callee: IrValue,
        args: Vec<IrValue>,
        is_optional: bool,
        spread_mask: Vec<bool>,
    },
    CallMethod {
        obj: IrValue,
        key: u16,
        args: Vec<IrValue>,
        is_optional: bool,
    },
    InvokeExtension {
        target: IrValue,
        name: Rc<str>,
        args: Vec<IrValue>,
    },

    IsType {
        val: IrValue,
        type_const: u16,
    },
    Debugger,
    Typeof(IrValue),
    ToString(IrValue),

    Sequence(Vec<IrValue>),
    TaggedTemplate {
        tag: IrValue,
        template_const: u16,
        values: Vec<IrValue>,
    },
    Range {
        start: IrValue,
        end: IrValue,
        inclusive: bool,
    },

    Import(u16),
    MergeExports {
        key: u16,
        val: IrValue,
    },
    Reexport(u16),
    ObjectKeys(IrValue),
    LoadFunction(usize),
    EndTry,
    CatchValue,
    BindMethod {
        method: IrValue,
        receiver: IrValue,
    },
    Raw(OpCode, IrOperand),
}

#[derive(Clone, Debug)]
pub enum IrPropKey {
    Const(u16),
    Computed(IrValue),
}

#[derive(Clone, Debug)]
pub enum IrTerminator {
    Return(Option<IrValue>),
    Throw(IrValue),
    Jump(usize),
    Branch {
        cond: IrValue,
        then_block: usize,
        else_block: usize,
    },
    Switch {
        discriminant: IrValue,
        cases: Vec<(IrValue, usize)>,
        default: usize,
    },
    Try {
        body: usize,
        catch_block: Option<(Option<IrValue>, usize)>,
        finally_block: Option<usize>,
    },
    Unreachable,
}

#[derive(Clone, Debug)]
pub struct IrStruct {
    pub name: Rc<str>,
    pub fields: Vec<(Rc<str>, Rc<str>)>,
}

#[derive(Clone, Debug)]
pub struct IrEnum {
    pub name: Rc<str>,
    pub variants: Vec<IrVariant>,
}

#[derive(Clone, Debug)]
pub struct IrVariant {
    pub name: Rc<str>,
    pub fields: Vec<Rc<str>>,
}

#[derive(Clone, Debug)]
pub enum IrOperand {
    None,
    One(u16),
    Two(u16, u16),
    Closure {
        function_const: u16,
        upvalues: Vec<IrUpvalueDesc>,
    },
}

#[derive(Clone, Debug)]
pub struct IrUpvalueDesc {
    pub is_local: bool,
    pub index: u16,
}
