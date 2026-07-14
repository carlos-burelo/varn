use std::rc::Rc;

use crate::error::{FrameInfo, RuntimeError, VmResult};
use crate::frame::{VmClosure, VmUpvalue};
use crate::value::VmValue;
use varn_types::{Literal, PoolEntry};

use super::calls::PreparedCall;
use super::ctx::ExecCtx;

impl ExecCtx {
    #[inline(always)]
    pub fn prepare_call(&mut self, callee_nv: VmValue, arg_count: usize) -> VmResult<PreparedCall> {
        if let Some((prepared, needs_receiver)) =
            super::calls::try_prepare_call_fast(callee_nv, arg_count, &self.stack, &self.heap)
        {
            if needs_receiver {
                if callee_nv.is_heap() {
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
                                if frame.base >= self.stack.len() {
                                    self.stack.push(recv_nv);
                                } else {
                                    self.stack[frame.base] = recv_nv;
                                }
                            }
                            PreparedCall::NativeImmediate(_, _)
                            | PreparedCall::RawNativeImmediate(_, _) => {
                                let args_start = self.stack.len() - arg_count;
                                if args_start >= self.stack.len() {
                                    self.stack.push(recv_nv);
                                } else {
                                    self.stack[args_start] = recv_nv;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            return Ok(prepared);
        }

        self.record_call_slow();
        let res = super::calls::prepare_call(
            callee_nv,
            arg_count,
            &mut self.stack,
            &mut self.heap,
            self.settings,
        );
        if let Err(ref e) = res {
            if let Some(f) = self.frames.last() {
                let code = &f.closure().proto.chunk.code;
                let ip = f.ip;
                let start = ip.saturating_sub(10);
                let end = (ip + 10).min(code.len());
                let code_snippet: Vec<(usize, u16)> = (start..end).map(|i| (i, code[i])).collect();
                let globals_dump: Vec<String> = self
                    .globals
                    .idx_to_name
                    .iter()
                    .enumerate()
                    .map(|(i, name)| {
                        let val = self
                            .globals
                            .values
                            .get(i)
                            .map(|v| self.heap.str_repr(*v))
                            .unwrap_or_else(|| "???".into());
                        format!("[{}]{}={}", i, name, val)
                    })
                    .collect();
                let frames_dump: Vec<String> = self
                    .frames
                    .iter()
                    .map(|fr| {
                        format!(
                            "{}@ip={}",
                            fr.closure().proto.name.as_deref().unwrap_or("<anon>"),
                            fr.ip
                        )
                    })
                    .collect();
                eprintln!("PREPARE_CALL ERROR: {:?}", e);
                eprintln!(
                    "  fn={:?}, file={}, ip={}",
                    f.closure().proto.name,
                    f.closure().proto.chunk.source_file,
                    f.ip
                );
                eprintln!("  code_near_ip={:?}", code_snippet);
                eprintln!("  frames={:?}", frames_dump);
                eprintln!("  globals={:?}", globals_dump);
            }
        }
        res
    }

    pub fn push_frame(&mut self, closure: Rc<VmClosure>) -> crate::error::VmResult<()> {
        if self.frames.len() >= 10000 {
            return Err(crate::error::RuntimeError::new(
                "stack overflow: call depth exceeded 10000",
            ));
        }
        let base = self.stack.len();
        let required = base + closure.proto.register_count as usize;
        if self.stack.len() < required {
            self.stack.resize(required, VmValue::null());
        }
        self.record_frame_push();
        self.frames
            .push(crate::frame::CallFrame::new_owned(closure, base));
        Ok(())
    }

    pub fn push_frame_at(&mut self, closure: Rc<VmClosure>, base: usize) {
        let required = base + closure.proto.register_count as usize;
        if self.stack.len() < required {
            self.stack.resize(required, VmValue::null());
        }
        self.record_frame_push();
        self.frames
            .push(crate::frame::CallFrame::new_owned(closure, base));
    }

    pub fn read_str_const_at(&self, idx: usize, frame_idx: usize) -> VmResult<Rc<str>> {
        let frame = &self.frames[frame_idx];
        match frame.proto().chunk.constants.get(idx) {
            Some(PoolEntry::Literal(Literal::Str(s))) => Ok(s.clone()),
            _ => Err(RuntimeError::new(format!(
                "constant {} is not a string",
                idx
            ))),
        }
    }

    pub fn capture_upvalue(&mut self, slot: usize) -> VmUpvalue {
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

    pub fn close_upvalues_above(&mut self, slot: usize) {
        if self.open_upvalues.is_empty() {
            return;
        }
        for (s, uv) in self.open_upvalues.iter().rev() {
            if *s >= slot {
                uv.close(&self.stack);
            }
        }
        self.open_upvalues.retain(|(s, _)| *s < slot);
    }

    pub fn capture_stack_trace(&self) -> Vec<FrameInfo> {
        let mut frames = Vec::with_capacity(self.frames.len());
        for frame in self.frames.iter().rev() {
            let proto = frame.proto();
            let fn_name = proto
                .name
                .clone()
                .unwrap_or_else(|| "<anonymous>".to_owned().into());
            let mut file = String::new();
            if let Some(mid) = &proto.chunk.module_id {
                file = mid.as_str().to_owned();
            } else if !proto.chunk.source_file.is_empty() {
                file = proto.chunk.source_file.to_string();
            }

            let line = proto.chunk.lines.get_line(frame.ip.saturating_sub(1));
            frames.push(FrameInfo {
                fn_name: fn_name.to_string(),
                file,
                line,
            });
        }
        frames
    }
}
