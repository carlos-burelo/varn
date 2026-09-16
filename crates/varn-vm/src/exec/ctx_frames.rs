use std::rc::Rc;

use crate::closure::{VmClosure, VmUpvalue};
use crate::error::VmResult;
use crate::frame_store::SlotAddr;
use crate::value::VmValue;

use super::calls::PreparedCall;
use super::ctx::ExecCtx;

impl ExecCtx {
    #[inline(always)]
    pub(crate) fn prepare_call(
        &mut self,
        callee_nv: VmValue,
        arg_count: usize,
    ) -> VmResult<PreparedCall> {
        if let Some((prepared, needs_receiver)) = super::calls::try_prepare_call_fast(
            callee_nv,
            arg_count,
            &self.stage,
            &self.heap,
            &mut self.stack,
        ) {
            if needs_receiver && callee_nv.is_heap() {
                let receiver_clone = if let Some(crate::heap::HeapObj::BoundMethod(bm)) =
                    self.heap.get(callee_nv.as_heap_idx())
                {
                    Some(bm.receiver.clone())
                } else {
                    None
                };
                if let Some(receiver) = receiver_clone {
                    let recv_nv = self.heap.intern(receiver);
                    match prepared {
                        PreparedCall::Frame(ref frame) => {
                            // El frame ya se materializó (r0 es DYN por
                            // construcción): el receiver ocupa su slot.
                            self.stack.unbox_into_reg(frame.base, 0, recv_nv)?;
                        }
                        PreparedCall::NativeImmediate(_, _)
                        | PreparedCall::RawNativeImmediate(_, _) => {
                            if self.stage.is_empty() {
                                self.stage.push(recv_nv);
                            } else {
                                self.stage[0] = recv_nv;
                            }
                        }
                        _ => {}
                    }
                }
            }
            return Ok(prepared);
        }

        self.record_call_slow();
        super::calls::prepare_call(
            callee_nv,
            arg_count,
            &mut self.stage,
            &mut self.heap,
            self.settings,
            &mut self.stack,
        )
    }

    pub(crate) fn push_frame(&mut self, closure: Rc<VmClosure>) -> crate::error::VmResult<()> {
        if self.frames.len() >= 10000 {
            return Err(crate::error::RuntimeError::new(
                "stack overflow: call depth exceeded 10000",
            ));
        }
        let alloc = self.stack.push_frame(&closure.proto);
        self.frames
            .push(crate::frame::CallFrame::new_owned(closure, alloc));
        Ok(())
    }

    /// Saca el resultado de staging (lo deja `prepare_call`/`dispatch` en los
    /// caminos sin frame: nativas, `PushValue`, generadores).
    pub(crate) fn stage_pop(&mut self) -> VmValue {
        self.stage.pop().unwrap_or(VmValue::null())
    }

    pub(crate) fn capture_upvalue(&mut self, slot: SlotAddr) -> VmUpvalue {
        for (s, uv) in &self.open_upvalues {
            if *s == slot {
                return uv.clone();
            }
        }
        let up = VmUpvalue::open(slot);
        self.open_upvalues.push((slot, up.clone()));
        self.open_upvalues.sort_by_key(|(s, _)| *s);
        up
    }

    /// Cierra los upvalues abiertos dentro de la activación `alloc`
    /// (retornos y unwind: cerrar ANTES de liberar, el close lee el slot).
    pub(crate) fn close_upvalues_in(&mut self, alloc: usize) {
        if self.open_upvalues.is_empty() {
            return;
        }
        let bases = self.stack.alloc_bases(alloc);
        for (s, uv) in self.open_upvalues.iter().rev() {
            if s.idx >= bases[s.class.index()] {
                uv.close(&self.stack);
            }
        }
        self.open_upvalues
            .retain(|(s, _)| s.idx < bases[s.class.index()]);
    }

    /// Cierra los upvalues de la activación `alloc` desde el registro
    /// `lowest` (`CloseUpvalue`: cierres de ámbito de bloque).
    pub(crate) fn close_upvalues_from_reg(&mut self, alloc: usize, lowest: usize) {
        if self.open_upvalues.is_empty() {
            return;
        }
        let mut to_close = Vec::new();
        for (s, _) in self.open_upvalues.iter() {
            if let Some(reg) = self.stack.reg_of_addr(alloc, *s) {
                if reg >= lowest {
                    to_close.push(*s);
                }
            }
        }
        for s in &to_close {
            if let Some((_, uv)) = self.open_upvalues.iter().find(|(a, _)| a == s) {
                uv.close(&self.stack);
            }
        }
        self.open_upvalues.retain(|(s, _)| !to_close.contains(s));
    }
}
