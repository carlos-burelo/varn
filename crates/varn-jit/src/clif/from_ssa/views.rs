//! Array views cached between safepoints.
//!
//! Reaching an array's elements from its SSA value means loading the value
//! from its home and resolving it to its payload (heap tag, generation, slot
//! tag) before the element buffer, length and representation can be read. All
//! of that is fixed while nothing can move the array (a collection), reshape
//! it (a write through the runtime's accessor, a `push`, a call) or relocate
//! the frame. So each array receiver gets a view — buffer, length and
//! representation — held in Cranelift variables: an access uses it when it is
//! set and fills it when it is not, and any instruction outside a small
//! allowlist of ones that can do none of those things clears every view.
//! Cranelift's SSA construction merges the views across blocks, so a loop
//! whose body stays within the allowlist resolves each array once.
//!
//! A cleared view is a zero buffer pointer, which a live array never has.

use std::collections::HashMap;

use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::ssa::{SsaBinOp, SsaOp, SsaProto, SsaUnOp};

/// The variables holding one receiver's view.
#[derive(Clone, Copy)]
pub(crate) struct View {
    pub data: Variable,
    pub len: Variable,
    pub disc: Variable,
}

/// Every array receiver's view.
pub(crate) struct Views {
    by_value: HashMap<u32, View>,
}

impl Views {
    /// Declare a view for every receiver of an element access in `ssa`,
    /// cleared. The builder must be in the entry block.
    pub(super) fn declare(b: &mut FunctionBuilder, ssa: &SsaProto) -> Self {
        let mut by_value = HashMap::new();
        for inst in ssa.blocks.iter().flat_map(|blk| &blk.insts) {
            let object = match &inst.op {
                SsaOp::ArrayGetIndex { object, .. } | SsaOp::ArraySetIndex { object, .. } => {
                    *object
                }
                _ => continue,
            };
            by_value.entry(object).or_insert_with(|| View {
                data: b.declare_var(types::I64),
                len: b.declare_var(types::I64),
                disc: b.declare_var(types::I64),
            });
        }
        let views = Views { by_value };
        views.clear(b);
        views
    }

    pub(super) fn of(&self, object: u32) -> Option<View> {
        self.by_value.get(&object).copied()
    }

    /// Forget every view, at a point where an array may have moved or
    /// changed shape.
    pub(super) fn clear(&self, b: &mut FunctionBuilder) {
        if self.by_value.is_empty() {
            return;
        }
        let zero = b.ins().iconst(types::I64, 0);
        for view in self.by_value.values() {
            b.def_var(view.data, zero);
            b.def_var(view.len, zero);
            b.def_var(view.disc, zero);
        }
    }
}

/// Whether `op` keeps every view valid: it cannot collect, call out, or
/// change an array's buffer, length or representation on its path through
/// compiled code. An element store's own slow path clears the views itself
/// (a cross-typed write reshapes the buffer); an overflowing `int` op throws
/// and never returns.
pub(super) fn keeps_views(ssa: &SsaProto, op: &SsaOp) -> bool {
    match op {
        SsaOp::ConstInt(_)
        | SsaOp::ConstFloat(_)
        | SsaOp::ConstBool(_)
        | SsaOp::ConstNull
        | SsaOp::Cast { .. }
        | SsaOp::LoadCaptured { .. }
        | SsaOp::StoreCaptured { .. }
        | SsaOp::ArrayGetIndex { .. }
        | SsaOp::ArraySetIndex { .. } => true,
        // Native arithmetic, comparisons and bitwise ops; an `int` overflow
        // throws. Division, modulo and power run a helper, a concatenation
        // allocates, a `Dyn` operator runs the generic runtime one.
        SsaOp::Binary { op, .. } => match op {
            SsaBinOp::IntAdd
            | SsaBinOp::IntSub
            | SsaBinOp::IntMul
            | SsaBinOp::IntEq
            | SsaBinOp::IntNe
            | SsaBinOp::IntLt
            | SsaBinOp::IntLe
            | SsaBinOp::IntGt
            | SsaBinOp::IntGe
            | SsaBinOp::IntAnd
            | SsaBinOp::IntOr
            | SsaBinOp::IntXor
            | SsaBinOp::IntShl
            | SsaBinOp::IntShr
            | SsaBinOp::IntUshr
            | SsaBinOp::FloatAdd
            | SsaBinOp::FloatSub
            | SsaBinOp::FloatMul
            | SsaBinOp::FloatDiv
            | SsaBinOp::FloatEq
            | SsaBinOp::FloatNe
            | SsaBinOp::FloatLt
            | SsaBinOp::FloatLe
            | SsaBinOp::FloatGt
            | SsaBinOp::FloatGe => true,
            SsaBinOp::IntDiv
            | SsaBinOp::IntMod
            | SsaBinOp::IntPow
            | SsaBinOp::FloatMod
            | SsaBinOp::FloatPow
            | SsaBinOp::StrConcat
            | SsaBinOp::Dyn(_) => false,
        },
        SsaOp::Unary { op, .. } => match op {
            SsaUnOp::NegInt | SsaUnOp::NegFloat | SsaUnOp::Not | SsaUnOp::BitNotInt => true,
            SsaUnOp::Dyn(_) => false,
        },
        SsaOp::Convert { operand, conv } => super::numeric::is_inline_convert(ssa, *operand, *conv),
        _ => false,
    }
}
