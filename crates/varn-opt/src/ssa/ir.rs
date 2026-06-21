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

use crate::hir::{HirBinOp, HirFunction, HirType, HirUnOp};

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
}

impl SsaFunc {
    pub fn block(&self, id: BlockId) -> &Block {
        &self.blocks[id.0 as usize]
    }

    pub fn value_ty(&self, v: Value) -> HirType {
        self.values[v.0 as usize].ty
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
    /// Template literal (`BuildStr`) over already-stringified parts.
    BuildStr { parts: Vec<Value> },
    /// A capture-free closure/arrow/nested fn → `LoadStaticFn` (the nested
    /// function is compiled to a proto constant at emit time). Closures that
    /// capture upvalues are not yet supported in SSA (the captured local needs a
    /// stable slot, which SSA renaming breaks).
    MakeClosure { func: Rc<HirFunction> },
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
