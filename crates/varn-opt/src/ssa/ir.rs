//! SSA IR — a per-function control-flow graph of basic blocks in SSA form.
//!
//! Representation: **block parameters** (Cranelift/MLIR style) instead of phi
//! nodes. A merge point declares parameters; every predecessor's terminator
//! passes matching arguments. This is isomorphic to phi nodes but keeps the
//! merge operands on the edges, which the Braun et al. on-the-fly construction
//! (`crate::ssa::build`) produces naturally.
//!
//! Scalar dataflow (params/locals → [`Value`]s) is in SSA form; effectful and
//! heap operations are ordinary instructions threaded through the blocks in
//! program order. Each [`Value`] carries its static [`HirType`].

use std::rc::Rc;

use rust_decimal::Decimal;

use crate::hir::{HirBinOp, HirFunction, HirType, HirUnOp, LocalId, HirUpvalueSrc};

/// A source variable being SSA-renamed: a parameter slot or a local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VarId {
    Param(u32),
    Local(LocalId),
}

/// An SSA value: defined exactly once, by an instruction or a block parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Value(pub u32);

/// A basic block, identified by its index into [`SsaFunc::blocks`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

/// Per-value metadata: its static type. Indexed by `Value.0`.
#[derive(Debug, Clone, Copy)]
pub struct ValueDef {
    pub ty: HirType,
}

/// A whole function in SSA form.
#[derive(Debug)]
pub struct SsaFunc {
    pub name: Rc<str>,
    /// Entry block; its parameters are the function parameters in order.
    pub entry: BlockId,
    pub blocks: Vec<Block>,
    /// `Value.0` → definition metadata.
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

    /// Replace every *use* of `old` with `new`: instruction operands and terminator
    /// values (condition, return value, block arguments).
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
                    InstKind::GetProperty { object, .. } => sub(object),
                    InstKind::GetIndex { object, index } => {
                        sub(object);
                        sub(index);
                    }
                    InstKind::SetProperty { object, value, .. } => {
                        sub(object);
                        sub(value);
                    }
                    InstKind::SetIndex { object, index, value } => {
                        sub(object);
                        sub(index);
                        sub(value);
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
                        if let Some(sc) = super_class { sub(sc); }
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
                    InstKind::DefineAccessor { class, accessor, .. } => {
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

/// A basic block: parameters (the SSA merge inputs), a straight-line list of
/// instructions, and a single terminator.
#[derive(Debug)]
pub struct Block {
    pub params: Vec<Value>,
    pub insts: Vec<Inst>,
    pub term: Terminator,
    /// Predecessor blocks, in the order edges were added (for block-arg slots).
    pub preds: Vec<BlockId>,
}

/// One instruction: an operation and the value it defines (if any).
#[derive(Debug, Clone)]
pub struct Inst {
    pub dest: Option<Value>,
    pub kind: InstKind,
}

/// The set of SSA operations. Mirrors the scalar/value-producing subset of
/// [`crate::hir::HirExpr`], flattened so operands are already-computed
/// [`Value`]s. Grown incrementally as construction coverage expands.
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
    /// Read a module global by name (`LoadGlobal`).
    LoadGlobal(Rc<str>),
    /// Read a captured upvalue by index (`LoadUpvalue`). The closure's upvalue
    /// array is populated by the parent's `MakeClosure`; reading one is a leaf
    /// (no SSA operands), so a closure *body* can be SSA-compiled even when the
    /// parent that builds the closure still falls back to `lower/`.
    LoadUpvalue(u32),
    /// Write a module global (`DefineGlobal`). Side effect, no `dest`. Treated as
    /// memory: emitted in program order, never reordered/CSE'd (no optimizer yet).
    StoreGlobal { name: Rc<str>, value: Value },
    /// Write a captured upvalue's shared cell (`StoreUpvalue`). Side effect, no
    /// `dest`; mutations are visible to every closure sharing the cell.
    StoreUpvalue { index: u32, value: Value },
    /// Plain call `callee(args)` — plain-call ABI (null receiver, contiguous args).
    Call { callee: Value, args: Vec<Value> },
    /// Statically-resolved self-recursion (`CallSelf`).
    SelfCall { args: Vec<Value> },
    /// `object.name` member read (`GetProperty` with an inline-cache slot).
    GetProperty { object: Value, name: Rc<str> },
    /// `object[index]` computed read (`GetIndex`).
    GetIndex { object: Value, index: Value },
    /// `object.name = value` write (`SetProperty` + IC slot). Side effect; the
    /// defining inst has no `dest` (the assigned value is the `value` operand).
    SetProperty { object: Value, name: Rc<str>, value: Value },
    /// `object[index] = value` write (`SetIndex`). Side effect, no `dest`.
    SetIndex { object: Value, index: Value, value: Value },
    /// `recv.name(args)` method call (`CallMethod` + IC slot). Receiver passed
    /// separately (not in the args block); args contiguous from `call_base`.
    MethodCall { recv: Value, name: Rc<str>, args: Vec<Value> },
    /// `IsNull` — truthy-bool of whether the operand is null (for `??`).
    IsNull { operand: Value },
    /// `[a, b, …]` array literal (`BuildArray`); elements emitted contiguously.
    BuildArray { elements: Vec<Value> },
    /// `{ k: v, … }` object literal (`BuildObject`); static keys + value props,
    /// each value register listed individually (no contiguity requirement).
    BuildObject { pairs: Vec<(Rc<str>, Value)> },
    /// `ToString` — stringify an interpolated template part.
    ToString { operand: Value },
    /// A template literal (`BuildStr`) over already-stringified parts.
    BuildStr { parts: Vec<Value> },
    /// A closure/arrow/nested fn → `LoadStaticFn` or `MakeClosure`.
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
    /// VM intrinsic (`Math.*` etc.) → `Intrinsic` opcode. Operands are
    /// `[object, args…]` contiguous; `wire_byte` selects the operation.
    IntrinsicCall { object: Value, args: Vec<Value>, wire_byte: u8 },
    /// `expr!` non-null assertion (`AssertNotNull`). Side effect; the value
    /// passes through (no `dest` — the asserted value is `operand`).
    AssertNotNull { operand: Value },
    /// `object?.name` optional member read (`GetPropertyMaybe`).
    GetPropertyMaybe { object: Value, name: Rc<str> },
    /// Module-slot read (`LoadModuleSlot`).
    ModuleSlot { object: Value, slot: u16 },
    /// `GetEnumTag` — the enum/result tag of a value (for the `?` try operator).
    GetEnumTag { operand: Value },
    /// `IsArray` — runtime array test (for `is Array` type tests).
    IsArray { operand: Value },
    /// The method receiver (`this`) — register 0 copied into a value.
    This,
    /// `start..end` / `start..=end` → `InvokeRuntimeStatic __range__`.
    Range { start: Value, end: Value, inclusive: bool },
    /// `ObjectKeys` — the key array of an object (for `for-in`).
    ObjectKeys { operand: Value },
    /// `GetSymbol` — fetch a well-known symbol method (`Symbol.iterator` /
    /// `Symbol.asyncIterator`) off an object, for the `for-of` protocol.
    GetSymbol { object: Value, is_async: bool },
    /// Call `callee` with `recv` as its sole argument (the iterator-protocol
    /// `iterable[Symbol.iterator]()` / `iterator.next()` shape): receiver at
    /// `call_base`, `Call` with arg_count 1.
    IterCall { callee: Value, recv: Value },
    /// `super` / `super.name` member read (`GetSuper`).
    GetSuper { name: Rc<str> },
    /// `super(args)` — superclass constructor call: `GetSuper "constructor"`
    /// then `Call` with `this` (reg 0) as the receiver + args.
    SuperCall { args: Vec<Value> },
    /// `super.name(args)` — superclass method call: `GetSuper name` (bound to
    /// `this`) then `Call` over the args (no separate receiver).
    SuperMethodCall { name: Rc<str>, args: Vec<Value> },
    /// Extension call `recv.m(args)` → `LoadGlobal(func)` as the callee with
    /// `recv` in the receiver slot (`this`), args following (same ABI as a
    /// `super` constructor call, different callee).
    ExtensionCall { func: Rc<str>, recv: Value, args: Vec<Value> },
    /// Plain call with spread args (`f(a, ...b)`) → `CallSpread`; each `true`
    /// marks a spread arg, `WrapSpread`'d into its slot before the call.
    CallSpread { callee: Value, args: Vec<(Value, bool)> },
    /// Array literal with spread/holes (`[a, ...b]`) → an empty `BuildArray` then
    /// `ArrayPush` (element) / `ArrayExtend` (spread) per item in order.
    BuildArraySpread { elements: Vec<(Value, bool)> },
    /// Object literal with spread (`{a: 1, ...b}`) → an empty `BuildObject` then
    /// `SetProperty` per static pair / `ObjectMerge` per spread (`None` key), in
    /// order.
    BuildObjectSpread { parts: Vec<(Option<Rc<str>>, Value)> },
}

/// How a block ends and transfers control. Branch/jump carry the block-argument
/// lists that feed the successor's parameters (the SSA merge operands).
#[derive(Debug, Clone)]
pub enum Terminator {
    Return(Option<Value>),
    /// `throw value` — unwinds; the block has no successors.
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
    /// Placeholder while a block is under construction / for unreachable tails.
    Unreachable,
}
