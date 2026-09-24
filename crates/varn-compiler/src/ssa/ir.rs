use std::sync::Arc;

use bigdecimal::BigDecimal as Decimal;

use crate::hir::{HirBinOp, HirType, HirUnOp, HirUpvalueSrc, LocalId};

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
    pub name: Arc<str>,

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
    ConstStr(Arc<str>),
    ConstChar(char),
    ConstDecimal(Decimal),
    /// Canonical base-10 digits of a `bigint` constant.
    ConstBigInt(Arc<str>),
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

    LoadGlobal(Arc<str>),

    /// Module-global read at a region-relative slot the checker numbered.
    LoadGlobalIdx(u32),

    /// Prelude / host global read at its absolute native-layout index.
    LoadNativeGlobalIdx(u32),

    LoadUpvalue(u32),

    StoreGlobal {
        name: Arc<str>,
        value: Value,
    },

    /// Module-global write at a region-relative slot the checker numbered.
    StoreGlobalIdx {
        slot: u32,
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
        name: Arc<str>,
    },

    GetFixedField {
        object: Value,
        slot: u16,
        /// Compact byte offset of the field from the instance payload start
        /// (`ClassLayout`), baked by the compiler. `0` for a `Slot` access
        /// (a dynamic/enum-payload field).
        offset: u32,
        tag: varn_core::FieldAccess,
    },

    GetIndex {
        object: Value,
        index: Value,
    },

    ArrayGetIndex {
        object: Value,
        index: Value,
    },

    MapGetIndex {
        object: Value,
        index: Value,
    },

    SetProperty {
        object: Value,
        name: Arc<str>,
        value: Value,
    },

    SetFixedField {
        object: Value,
        value: Value,
        slot: u16,
        /// Compact byte offset of the field from the instance payload start.
        offset: u32,
        /// The field's kind (width for codegen); always a compact class field.
        tag: Option<varn_core::RuntimeKind>,
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

    MapSetIndex {
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
        name: Arc<str>,
        args: Vec<Value>,
    },

    IsNull {
        operand: Value,
    },

    Cast {
        operand: Value,
        ty: HirType,
    },

    /// Explicit numeric conversion (`as`) that changes representation.
    Convert {
        operand: Value,
        conv: varn_core::NumConv,
    },

    BuildArray {
        elements: Vec<Value>,
    },

    BuildTuple {
        elements: Vec<Value>,
    },

    BuildObject {
        pairs: Vec<(Arc<str>, Value)>,
    },

    BuildRecord {
        pairs: Vec<(Arc<str>, Value)>,
    },

    BuildMap {
        pairs: Vec<(Value, Value)>,
    },

    ObjectRest {
        object: Value,
        skip_keys: Vec<Arc<str>>,
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
        /// Index into `varn_tir::TirModule::functions` — the closure body.
        func: u32,
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
        name: Arc<str>,
        super_class: Option<Value>,
    },
    DeclareField {
        class: Value,
        name: Arc<str>,
        /// Static type the runtime lays this field out by.
        tag: Option<varn_core::RuntimeKind>,
    },
    DefineStatic {
        class: Value,
        name: Arc<str>,
        value: Value,
    },
    DefineMethod {
        class: Value,
        name: Arc<str>,
        method: Value,
        is_static: bool,
    },
    DefineAccessor {
        class: Value,
        name: Arc<str>,
        accessor: Value,
        is_getter: bool,
        is_static: bool,
    },
    MakeEnumVariant {
        tag: i64,
        meta: Arc<str>,
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
        source: Arc<str>,
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
        name: Arc<str>,
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

    /// `s.length` on a receiver statically typed `str`: an `int`. The one
    /// place a `length` read is specialised; every later stage maps it 1:1.
    StrLength {
        operand: Value,
    },

    /// `a.length` on a receiver statically typed as an array: an `int`.
    ArrayLength {
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
        name: Arc<str>,
    },

    SuperCall {
        args: Vec<Value>,
    },

    SuperMethodCall {
        name: Arc<str>,
        args: Vec<Value>,
    },

    ExtensionCall {
        func: Arc<str>,
        /// Module-global slot of the mangled function, when it was numbered
        /// (always, for a well-formed module). `None` falls back to a name load.
        slot: Option<u32>,
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
        parts: Vec<(Option<Arc<str>>, Value)>,
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
