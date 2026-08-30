use std::rc::Rc;

use rust_decimal::Decimal;

use crate::hir::{HirBinOp, HirFunction, HirType, HirUnOp, HirUpvalueSrc, LocalId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VarId {
    Param(u32),
    Local(LocalId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Value(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, Copy)]
pub struct ValueDef {
    pub ty: HirType,
}

#[derive(Debug)]
pub struct SsaFunc {
    pub name: Rc<str>,

    pub entry: BlockId,
    pub blocks: Vec<Block>,

    pub values: Vec<ValueDef>,
    pub pinned_vars: rustc_hash::FxHashSet<VarId>,
    pub nlocals: u32,

    // Forma DECLARADA de la función, propagada desde HIR (`is_async` e
    // `is_generator` debajo) — no dice si el cuerpo suspende de verdad. El
    // top-level de un módulo (`<module>`) es SIEMPRE `is_async: false` aunque
    // contenga `await` de nivel superior: HIR no le da forma `async` al
    // top-level, así que este campo no lo refleja. Un consumidor que use
    // `is_async`/`is_generator` como puerta para decidir si un cuerpo puede
    // suspender se saltaría ese caso. La fuente de verdad sobre suspensión
    // real, con los puntos concretos, es `crate::ssa::suspend::analyze`.
    /// Si la función se DECLARÓ `async`. No implica que el cuerpo suspenda
    /// (ver nota arriba) ni lo contrario. Ver también `is_generator`.
    pub is_async: bool,
    /// Si la función se DECLARÓ `function*` (generadora). Misma salvedad que
    /// `is_async` arriba: forma declarada, no comportamiento real del cuerpo.
    pub is_generator: bool,
}

impl SsaFunc {
    #[inline]
    pub fn block(&self, id: BlockId) -> &Block {
        &self.blocks[id.0 as usize]
    }

    #[inline]
    pub fn block_mut(&mut self, id: BlockId) -> &mut Block {
        &mut self.blocks[id.0 as usize]
    }

    #[inline]
    pub fn alloc_value(&mut self, ty: HirType) -> Value {
        let v = Value(self.values.len() as u32);
        self.values.push(ValueDef { ty });
        v
    }

    #[inline]
    pub fn alloc_block(&mut self) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(Block {
            params: Vec::new(),
            insts: Vec::new(),
            term: Terminator::Unreachable,
            term_line: 0,
            preds: Vec::new(),
        });
        id
    }

    pub fn value_ty(&self, v: Value) -> HirType {
        self.values[v.0 as usize].ty
    }

    /// Rewrites every read of `old` to `new`.
    ///
    /// Walks the whole function, so a pass with many rewrites should batch
    /// them through [`crate::ssa::uses::replace_uses_with_map`] instead.
    pub fn replace_all_uses(&mut self, old: Value, new: Value) {
        let mut sub = |v: &mut Value| {
            if *v == old {
                *v = new;
            }
        };
        for block in &mut self.blocks {
            for inst in &mut block.insts {
                crate::ssa::uses::visit_uses_mut(&mut inst.kind, &mut sub);
            }
            crate::ssa::uses::visit_term_uses_mut(&mut block.term, &mut sub);
        }
    }
}

#[derive(Debug)]
pub struct Block {
    pub params: Vec<Value>,
    pub insts: Vec<Inst>,
    pub term: Terminator,
    pub term_line: u32,

    pub preds: Vec<BlockId>,
}

#[derive(Debug, Clone)]
pub struct Inst {
    pub dest: Option<Value>,
    pub kind: InstKind,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub enum InstKind {
    ConstInt(i64),
    ConstFloat(f64),
    ConstBool(bool),
    ConstStr(Rc<str>),
    ConstChar(char),
    ConstDecimal(Decimal),
    ConstBigInt(i128),
    ConstNull,
    Binary {
        op: HirBinOp,
        lhs: Value,
        rhs: Value,
        ty: HirType,
    },
    Unary {
        op: HirUnOp,
        operand: Value,
        ty: HirType,
    },

    LoadGlobal(Rc<str>),

    LoadUpvalue(u32),

    StoreGlobal {
        name: Rc<str>,
        value: Value,
    },

    StoreUpvalue {
        index: u32,
        value: Value,
    },

    Call {
        callee: Value,
        args: Vec<Value>,
    },

    SelfCall {
        args: Vec<Value>,
    },

    GetProperty {
        object: Value,
        name: Rc<str>,
    },

    GetFixedField {
        object: Value,
        slot: u16,
    },

    GetIndex {
        object: Value,
        index: Value,
    },

    ArrayGetIndex {
        object: Value,
        index: Value,
    },

    SetProperty {
        object: Value,
        name: Rc<str>,
        value: Value,
    },

    SetFixedField {
        object: Value,
        value: Value,
        slot: u16,
    },

    SetIndex {
        object: Value,
        index: Value,
        value: Value,
    },

    ArraySetIndex {
        object: Value,
        index: Value,
        value: Value,
    },

    /// `arr.push(v)` con receptor probado como array y resultado descartado.
    /// La nativa `push` no devuelve nada, así que como sentencia siempre
    /// puede tomar el opcode dedicado.
    ArrayPush {
        array: Value,
        value: Value,
    },

    ObjectMerge {
        target: Value,
        source: Value,
    },

    MethodCall {
        recv: Value,
        name: Rc<str>,
        args: Vec<Value>,
    },

    IsNull {
        operand: Value,
    },

    BuildArray {
        elements: Vec<Value>,
    },

    BuildTuple {
        elements: Vec<Value>,
    },

    BuildObject {
        pairs: Vec<(Rc<str>, Value)>,
    },

    BuildRecord {
        pairs: Vec<(Rc<str>, Value)>,
    },

    ObjectRest {
        object: Value,
        skip_keys: Vec<Rc<str>>,
    },

    ToString {
        operand: Value,
    },

    BuildStr {
        parts: Vec<Value>,
    },

    /// Las capturas se describen SÓLO por origen (`upvalues_src`): el
    /// descriptor emitido nombra el slot canónico del frame padre
    /// (`var_reg`) o un índice de upvalue heredada, nunca un `Value`.
    /// Listar aquí los valores capturados creaba operandos fantasma que el
    /// backend materializaba en `Move` que nadie lee.
    MakeClosure {
        func: Rc<HirFunction>,
        upvalues_src: Vec<HirUpvalueSrc>,
    },
    LoadCaptured {
        var: VarId,
    },
    StoreCaptured {
        var: VarId,
        value: Value,
    },
    MakeClass {
        name: Rc<str>,
        super_class: Option<Value>,
    },
    DeclareField {
        class: Value,
        name: Rc<str>,
    },
    DefineStatic {
        class: Value,
        name: Rc<str>,
        value: Value,
    },
    DefineMethod {
        class: Value,
        name: Rc<str>,
        method: Value,
        is_static: bool,
    },
    DefineAccessor {
        class: Value,
        name: Rc<str>,
        accessor: Value,
        is_getter: bool,
        is_static: bool,
    },
    MakeEnumVariant {
        tag: i64,
        meta: Rc<str>,
    },
    Try {
        handler: BlockId,
    },
    PopTry,
    CatchParam {
        try_val: Value,
    },
    CloseUpvalues {
        targets: Vec<VarId>,
    },
    Dispose {
        target: LocalId,
        is_await: bool,
    },
    LoadModule {
        source: Rc<str>,
    },
    StoreModuleSlot {
        value: Value,
        slot: u16,
    },
    Await {
        operand: Value,
    },
    Spawn {
        operand: Value,
    },
    Yield {
        operand: Value,
    },

    IntrinsicCall {
        object: Value,
        args: Vec<Value>,
        wire_byte: u8,
    },

    CallNativeOp {
        object: Value,
        args: Vec<Value>,
        op_id: u64,
    },

    AssertNotNull {
        operand: Value,
    },

    GetPropertyMaybe {
        object: Value,
        name: Rc<str>,
    },

    ModuleSlot {
        object: Value,
        slot: u16,
    },

    GetEnumTag {
        operand: Value,
    },

    IsArray {
        operand: Value,
    },

    This,

    Range {
        start: Value,
        end: Value,
        inclusive: bool,
    },

    ObjectKeys {
        operand: Value,
    },

    GetSymbol {
        object: Value,
        is_async: bool,
    },

    IterCall {
        callee: Value,
        recv: Value,
    },

    GetSuper {
        name: Rc<str>,
    },

    SuperCall {
        args: Vec<Value>,
    },

    SuperMethodCall {
        name: Rc<str>,
        args: Vec<Value>,
    },

    ExtensionCall {
        func: Rc<str>,
        recv: Value,
        args: Vec<Value>,
    },

    CallSpread {
        callee: Value,
        args: Vec<(Value, bool)>,
    },

    BuildArraySpread {
        elements: Vec<(Value, bool)>,
    },

    BuildObjectSpread {
        parts: Vec<(Option<Rc<str>>, Value)>,
    },
}

#[derive(Debug, Clone)]
pub enum Terminator {
    Return(Option<Value>),

    Throw(Value),
    Jump {
        target: BlockId,
        args: Vec<Value>,
    },
    Branch {
        cond: Value,
        then_blk: BlockId,
        then_args: Vec<Value>,
        else_blk: BlockId,
        else_args: Vec<Value>,
    },

    Unreachable,
}
