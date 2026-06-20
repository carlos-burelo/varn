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

use crate::hir::{HirBinOp, HirType, HirUnOp};

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
    /// Plain call `callee(args)` — plain-call ABI (null receiver, contiguous args).
    Call { callee: Value, args: Vec<Value> },
    /// Statically-resolved self-recursion (`CallSelf`).
    SelfCall { args: Vec<Value> },
}

/// How a block ends and transfers control. Branch/jump carry the block-argument
/// lists that feed the successor's parameters (the SSA merge operands).
#[derive(Debug, Clone)]
pub enum Terminator {
    Return(Option<Value>),
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
