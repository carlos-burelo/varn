


use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::FunctionProto;


#[derive(Clone, Copy)]
pub(crate) enum UpvalueSrc {
    
    
    Local(usize),
    
    Inherited(usize),
}

impl UpvalueSrc {
    
    pub(crate) fn from_bytecode(word: u16) -> Self {
        let index = (word & 0xFF) as usize;
        if word >> 8 != 0 {
            UpvalueSrc::Local(index)
        } else {
            UpvalueSrc::Inherited(index)
        }
    }

    
    
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
    pub(crate) fn shared_constants(&mut self, proto: &Rc<FunctionProto>) -> Rc<Vec<VmValue>> {
        let key = Rc::as_ptr(proto) as usize;
        Rc::clone(
            &unsafe { &mut *self.proto_constants.get() }
                .entry(key)
                .or_insert_with(|| {
                    let resolved =
                        Rc::new(crate::exec::calls::resolve_constants(proto, &mut self.heap));
                    (Rc::clone(proto), resolved)
                })
                .1,
        )
    }

    
    
    
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
            if let Some(&(_, cached)) = unsafe { &*self.static_closures.get() }.get(&proto_ptr) {
                return Ok(cached);
            }
        }
        let captured = upvalues
            .map(|src| match src {
                UpvalueSrc::Local(reg) => self.capture_upvalue(self.stack.addr_of(base, reg)),
                UpvalueSrc::Inherited(idx) => parent.upvalues[idx].clone(),
            })
            .collect();
        let constants = self.shared_constants(&proto);
        let mut closure =
            VmClosure::with_upvalues(proto.clone(), captured, constants, self.settings);
        
        closure.module_base = parent.module_base;
        let val = self.heap.alloc_vm_closure(Rc::new(closure));
        if is_static {
            unsafe { &mut *self.static_closures.get() }.insert(proto_ptr, (proto, val));
        }
        Ok(val)
    }
}
