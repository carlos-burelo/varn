use crate::error::VmResult;
use crate::frame_store::FrameStore;
use crate::value::VmValue;

#[derive(Clone, Copy)]
pub(crate) enum MethodArgs<'a> {
    Regs {
        base: usize,
        start: usize,
        count: usize,
    },

    Boxed(&'a [VmValue]),
}

impl MethodArgs<'_> {
    pub(crate) fn len(&self) -> usize {
        match self {
            MethodArgs::Regs { count, .. } => *count,
            MethodArgs::Boxed(values) => values.len(),
        }
    }

    pub(crate) fn get(&self, stack: &FrameStore, i: usize) -> VmValue {
        match self {
            MethodArgs::Regs { base, start, .. } => stack.box_reg(*base, start + i),
            MethodArgs::Boxed(values) => values[i],
        }
    }

    pub(crate) fn copy_into(
        &self,
        stack: &mut FrameStore,
        alloc: usize,
        dst: usize,
        n: usize,
    ) -> VmResult<()> {
        for i in 0..n {
            match self {
                MethodArgs::Regs { base, start, .. } => {
                    stack.mov_cross(alloc, dst + i, *base, start + i)?
                }
                MethodArgs::Boxed(values) => stack.unbox_into_reg(alloc, dst + i, values[i])?,
            }
        }
        Ok(())
    }
}

pub(crate) enum MethodOutcome {
    Value(VmValue),
    FramePushed,
}
