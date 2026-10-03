use cranelift_codegen::ir::{types, InstBuilder, StackSlotData, StackSlotKind, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::ssa::SsaProto;

use super::super::Ctx;

pub(crate) struct ScratchWin {
    addr: Value,
    max: usize,
}

impl ScratchWin {
    pub(crate) fn create(b: &mut FunctionBuilder, max: usize) -> Option<ScratchWin> {
        if max == 0 {
            return None;
        }
        let slot = b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (max * 16) as u32,
            4,
        ));
        Some(ScratchWin {
            addr: b.ins().stack_addr(types::I64, slot, 0),
            max,
        })
    }
}

pub(crate) fn scratch_max(ssa: &SsaProto) -> usize {
    use varn_types::ssa::SsaOp;
    let mut max = 0usize;
    for blk in &ssa.blocks {
        for inst in &blk.insts {
            let need = match &inst.op {
                SsaOp::Call { args, .. }
                | SsaOp::SelfCall { args }
                | SsaOp::SuperCall { args }
                | SsaOp::MethodCall { args, .. } => args.len() + 1,
                SsaOp::CallNativeOp { args, .. }
                | SsaOp::SuperMethodCall { args, .. }
                | SsaOp::ExtensionCall { args, .. } => args.len() + 1,
                SsaOp::CallSpread { args, .. } => args.len(),
                SsaOp::New { args, .. } => args.len(),
                SsaOp::IterCall { .. } => 2,
                SsaOp::Dispose { .. } => 1,
                SsaOp::IntrinsicCall { args, .. } => args.len() + 1,
                SsaOp::BuildStr { parts } | SsaOp::BuildTuple { elements: parts } => parts.len(),
                SsaOp::BuildArray { elements } => elements.len(),
                SsaOp::BuildMap { pairs } => pairs.len() * 2,
                SsaOp::ObjectRest { skip_keys, .. } => skip_keys.len(),
                _ => 0,
            };
            max = max.max(need);
        }
    }
    max
}

pub(crate) fn scratch_addr(b: &mut FunctionBuilder, ctx: &Ctx<'_>, count: usize) -> Value {
    if let Some(s) = &ctx.scratch {
        if count <= s.max {
            return s.addr;
        }
    }
    let slot = b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (count.max(1) * 16) as u32,
        4,
    ));
    b.ins().stack_addr(types::I64, slot, 0)
}
