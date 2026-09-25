//! Creating a closure: the one implementation behind the interpreter's
//! `MakeClosure` / `LoadStaticFn` and both JIT lowerings.

use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use std::rc::Rc;

/// Where one of a new closure's upvalues comes from.
#[derive(Clone, Copy)]
pub(crate) enum UpvalueSrc {
    /// Register `reg` of the creating activation: captured (shared while the
    /// activation lives, closed when it leaves).
    Local(usize),
    /// The creating closure's own upvalue `idx`.
    Inherited(usize),
}

impl UpvalueSrc {
    /// The bytecode descriptor word: `is_local << 8 | index`.
    pub(crate) fn from_bytecode(word: u16) -> Self {
        let index = (word & 0xFF) as usize;
        if word >> 8 != 0 {
            UpvalueSrc::Local(index)
        } else {
            UpvalueSrc::Inherited(index)
        }
    }

    /// The word compiled code passes (`varn_types::ssa::UPVALUE_LOCAL` set
    /// for a register, the index in the low 32 bits).
    pub(crate) fn from_word(word: u64) -> Self {
        let index = (word & 0xFFFF_FFFF) as usize;
        if word & varn_types::ssa::UPVALUE_LOCAL != 0 {
            UpvalueSrc::Local(index)
        } else {
            UpvalueSrc::Inherited(index)
        }
    }
}

impl ExecCtx {
    /// The closure of `parent`'s function constant `proto_idx`, capturing
    /// `upvalues` from activation `base` and `parent`. A closure without
    /// upvalues is created once per function and reused.
    pub(crate) fn make_closure(
        &mut self,
        parent: &VmClosure,
        proto_idx: usize,
        base: usize,
        upvalues: impl ExactSizeIterator<Item = UpvalueSrc>,
    ) -> VmResult<VmValue> {
        let proto = match parent.proto.chunk.constants.get(proto_idx) {
            Some(varn_types::PoolEntry::Function(p)) => p.clone(),
            _ => {
                return Err(RuntimeError::new(format!(
                    "MakeClosure: const {proto_idx} is not a function"
                )))
            }
        };
        let proto_ptr = Rc::as_ptr(&proto) as usize;
        let is_static = upvalues.len() == 0;
        if is_static {
            if let Some(&(_, cached)) = self.static_closures.get(&proto_ptr) {
                return Ok(cached);
            }
        }
        let captured = upvalues
            .map(|src| match src {
                UpvalueSrc::Local(reg) => self.capture_upvalue(self.stack.addr_of(base, reg)),
                UpvalueSrc::Inherited(idx) => parent.upvalues[idx].clone(),
            })
            .collect();
        let constants = self
            .proto_constants
            .entry(proto_ptr)
            .or_insert_with(|| {
                let resolved = Rc::new(crate::exec::calls::resolve_constants(
                    &proto,
                    &mut self.heap,
                ));
                (proto.clone(), resolved)
            })
            .1
            .clone();
        let mut closure =
            VmClosure::with_upvalues(proto.clone(), captured, constants, self.settings);
        // A nested closure runs against its defining module's globals.
        closure.module_base = parent.module_base;
        let val = self.heap.alloc_vm_closure(Rc::new(closure));
        if is_static {
            self.static_closures.insert(proto_ptr, (proto, val));
        }
        Ok(val)
    }
}
