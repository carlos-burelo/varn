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
}

impl SsaFunc {
    pub fn block(&self, id: BlockId) -> &Block {
        &self.blocks[id.0 as usize]
    }

    pub fn value_ty(&self, v: Value) -> HirType {
        self.values[v.0 as usize].ty
    }

    pub fn replace_all_uses(&mut self, old: Value, new: Value) {
        let sub = |v: &mut Value| {
            if *v == old {
                *v = new;
            }
        };
        for block in &mut self.blocks {
            for inst in &mut block.insts {
                match &mut inst.kind {
                    InstKind::Binary { lhs, rhs, .. } => {
                        sub(lhs);
                        sub(rhs);
                    }
                    InstKind::Unary { operand, .. } => sub(operand),
                    InstKind::Call { callee, args } => {
                        sub(callee);
                        args.iter_mut().for_each(sub);
                    }
                    InstKind::SelfCall { args } => args.iter_mut().for_each(sub),
                    InstKind::GetProperty { object, .. }
                    | InstKind::GetFixedField { object, .. } => sub(object),
                    InstKind::GetIndex { object, index }
                    | InstKind::ArrayGetIndex { object, index } => {
                        sub(object);
                        sub(index);
                    }
                    InstKind::SetProperty { object, value, .. }
                    | InstKind::SetFixedField { object, value, .. } => {
                        sub(object);
                        sub(value);
                    }
                    InstKind::SetIndex {
                        object,
                        index,
                        value,
                    }
                    | InstKind::ArraySetIndex {
                        object,
                        index,
                        value,
                    } => {
                        sub(object);
                        sub(index);
                        sub(value);
                    }
                    InstKind::ObjectMerge { target, source } => {
                        sub(target);
                        sub(source);
                    }
                    InstKind::MethodCall { recv, args, .. } => {
                        sub(recv);
                        args.iter_mut().for_each(sub);
                    }
                    InstKind::IsNull { operand } => sub(operand),
                    InstKind::BuildArray { elements } => elements.iter_mut().for_each(sub),
                    InstKind::BuildObject { pairs } => pairs.iter_mut().for_each(|(_, v)| sub(v)),
                    InstKind::ToString { operand } => sub(operand),
                    InstKind::BuildStr { parts } => parts.iter_mut().for_each(sub),
                    InstKind::MakeClosure { upvalues, .. } => {
                        upvalues.iter_mut().for_each(sub);
                    }
                    InstKind::IntrinsicCall { object, args, .. } => {
                        sub(object);
                        args.iter_mut().for_each(sub);
                    }
                    InstKind::CallNativeOp { object, args, .. } => {
                        sub(object);
                        args.iter_mut().for_each(sub);
                    }
                    InstKind::AssertNotNull { operand } => sub(operand),
                    InstKind::GetPropertyMaybe { object, .. } => sub(object),
                    InstKind::ModuleSlot { object, .. } => sub(object),
                    InstKind::GetEnumTag { operand } => sub(operand),
                    InstKind::IsArray { operand } => sub(operand),
                    InstKind::This => {}
                    InstKind::Range { start, end, .. } => {
                        sub(start);
                        sub(end);
                    }
                    InstKind::ObjectKeys { operand } => sub(operand),
                    InstKind::GetSymbol { object, .. } => sub(object),
                    InstKind::IterCall { callee, recv } => {
                        sub(callee);
                        sub(recv);
                    }
                    InstKind::SuperCall { args } => args.iter_mut().for_each(sub),
                    InstKind::SuperMethodCall { args, .. } => args.iter_mut().for_each(sub),
                    InstKind::ExtensionCall { recv, args, .. } => {
                        sub(recv);
                        args.iter_mut().for_each(sub);
                    }
                    InstKind::CallSpread { callee, args } => {
                        sub(callee);
                        args.iter_mut().for_each(|(v, _)| sub(v));
                    }
                    InstKind::BuildArraySpread { elements } => {
                        elements.iter_mut().for_each(|(v, _)| sub(v));
                    }
                    InstKind::BuildObjectSpread { parts } => {
                        parts.iter_mut().for_each(|(_, v)| sub(v));
                    }
                    InstKind::StoreGlobal { value, .. } => sub(value),
                    InstKind::StoreUpvalue { value, .. } => sub(value),
                    InstKind::LoadCaptured { .. } => {}
                    InstKind::StoreCaptured { value, .. } => sub(value),
                    InstKind::MakeClass { super_class, .. } => {
                        if let Some(sc) = super_class {
                            sub(sc);
                        }
                    }
                    InstKind::DeclareField { class, .. } => sub(class),
                    InstKind::DefineStatic { class, value, .. } => {
                        sub(class);
                        sub(value);
                    }
                    InstKind::DefineMethod { class, method, .. } => {
                        sub(class);
                        sub(method);
                    }
                    InstKind::DefineAccessor {
                        class, accessor, ..
                    } => {
                        sub(class);
                        sub(accessor);
                    }
                    InstKind::MakeEnumVariant { .. } => {}
                    InstKind::Try { .. } => {}
                    InstKind::PopTry => {}
                    InstKind::CatchParam { try_val } => sub(try_val),
                    InstKind::CloseUpvalues { .. } => {}
                    InstKind::Dispose { .. } => {}
                    InstKind::LoadModule { .. } => {}
                    InstKind::StoreModuleSlot { value, .. } => sub(value),
                    InstKind::Await { operand } => sub(operand),
                    InstKind::Spawn { operand } => sub(operand),
                    InstKind::Yield { operand } => sub(operand),
                    InstKind::GetSuper { .. }
                    | InstKind::ConstInt(_)
                    | InstKind::ConstFloat(_)
                    | InstKind::ConstBool(_)
                    | InstKind::ConstStr(_)
                    | InstKind::ConstChar(_)
                    | InstKind::ConstDecimal(_)
                    | InstKind::ConstBigInt(_)
                    | InstKind::ConstNull
                    | InstKind::LoadGlobal(_)
                    | InstKind::LoadUpvalue(_) => {}
                }
            }
            match &mut block.term {
                Terminator::Return(Some(v)) => sub(v),
                Terminator::Throw(v) => sub(v),
                Terminator::Return(None) | Terminator::Unreachable => {}
                Terminator::Jump { args, .. } => args.iter_mut().for_each(sub),
                Terminator::Branch {
                    cond,
                    then_args,
                    else_args,
                    ..
                } => {
                    sub(cond);
                    then_args.iter_mut().for_each(sub);
                    else_args.iter_mut().for_each(sub);
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct Block {
    pub params: Vec<Value>,
    pub insts: Vec<Inst>,
    pub term: Terminator,

    pub preds: Vec<BlockId>,
}

#[derive(Debug, Clone)]
pub struct Inst {
    pub dest: Option<Value>,
    pub kind: InstKind,
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

    BuildObject {
        pairs: Vec<(Rc<str>, Value)>,
    },

    ToString {
        operand: Value,
    },

    BuildStr {
        parts: Vec<Value>,
    },

    MakeClosure {
        func: Rc<HirFunction>,
        upvalues: Vec<Value>,
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
