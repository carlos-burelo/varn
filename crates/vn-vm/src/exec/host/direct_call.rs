use std::ffi::c_void;
use std::rc::Rc;

use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::frame::{CallFrame, MAX_CALL_DEPTH};
use crate::value::VmValue;
use vn_types::register_meta::SlotKind;

const MAX_WINDOW: usize = 8;

type HostThunk = unsafe extern "C" fn(*mut c_void, *const c_void, *const VmValue, *mut VmValue);

fn kind_accepts(kind: SlotKind, arg: VmValue) -> bool {
    match kind {
        SlotKind::Int => arg.is_int(),
        SlotKind::Bool => arg.is_bool(),
        SlotKind::Float => arg.is_f64() || arg.is_int(),
        SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic => true,
    }
}

impl ExecCtx {
    pub(crate) fn call_compiled_direct(
        &mut self,
        callee: VmValue,
        args: &[VmValue],
    ) -> Option<VmResult<VmValue>> {
        let closure = Rc::clone(self.heap.closure_of(callee)?);
        let proto = &closure.proto;
        let host = proto.jit_host.get();
        if host == 0
            || proto.arity != args.len() + 1
            || args.len() >= MAX_WINDOW
            || !args
                .iter()
                .zip(proto.param_kinds.iter())
                .all(|(arg, kind)| kind_accepts(*kind, *arg))
        {
            return None;
        }
        if crate::frame::native_stack_low(self.jit_stack_floor) {
            return Some(Err(crate::frame::native_stack_exhausted()));
        }
        if self.call_depth() >= MAX_CALL_DEPTH {
            return Some(Err(crate::frame::call_depth_exceeded()));
        }
        let mut window = [VmValue::null(); MAX_WINDOW];
        window[0] = callee;
        window[1..=args.len()].copy_from_slice(args);

        let was_native = self.heap.cells.suspend_native();
        self.frames
            .push(CallFrame::new_owned(Rc::clone(&closure), CallFrame::NO_ACTIVATION));
        let exits_len = self.jit_exits_saved.len();
        let frameless_depth = self.jit_frameless_depth;
        self.jit_exits_saved.push(self.jit_exit);
        self.jit_exit = vn_jit::stack_roots::JitExit::default();
        if self.jit_error_flag == 0 {
            self.jit_panic_exception_error = None;
            self.jit_panic_exception_err_obj = None;
        }
        let mut out = VmValue::null();
        let thunk: HostThunk = unsafe { std::mem::transmute(host) };
        unsafe {
            thunk(
                self as *mut ExecCtx as *mut c_void,
                Rc::as_ptr(&closure) as *const c_void,
                window.as_ptr(),
                &mut out,
            );
        }
        self.jit_exit = self.jit_exits_saved[exits_len];
        self.jit_exits_saved.truncate(exits_len);
        self.jit_frameless_depth = frameless_depth;

        let result = if self.jit_error_flag != 0 {
            Err(self.take_native_failure())
        } else {
            self.frames.pop();
            Ok(out)
        };
        let returned = result
            .as_ref()
            .ok()
            .filter(|v| v.is_heap())
            .map(|v| v.as_heap());
        self.heap.cells.resume(was_native, returned);
        Some(result)
    }

    fn take_native_failure(&mut self) -> crate::error::RuntimeError {
        self.jit_error_flag = 0;
        let thrown = self.jit_native_result;
        let stored_obj = self.jit_panic_exception_err_obj.take();
        let stored_val = self.jit_panic_exception_error.take();
        let (error_val, err_obj) = match stored_val {
            Some(val) if val == thrown => (val, stored_obj),
            Some(_) | None => (thrown, None),
        };
        if let Some(frame) = self.frames.pop() {
            self.drop_frame_storage(frame.base);
        }
        self.try_handlers
            .retain(|h| h.frame_depth <= self.frames.len());
        match err_obj {
            Some(error) => error,
            None => crate::exec::exceptions::build_thrown_error(
                error_val,
                &self.heap,
                &self.frames,
            ),
        }
    }
}
