use crate::error::{FrameInfo, RuntimeError, VmResult};
use crate::frame::{CallFrame, TryHandler, VmClosure, VmUpvalue};
use crate::globals::GlobalStore;
use crate::heap::{Heap, HeapObj};
use crate::loader::ModuleLoader;
use crate::profile::ProfileCounters;
use crate::value::VmValue;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use varn_core::ModuleId;
use varn_core::OpCode;
use varn_types::value::LazyTask;
use varn_types::{FunctionProto, Literal, NativeCtx, PoolEntry, Value};

use super::calls::{self, PreparedCall};
use super::VmSuspend;
use varn_types::generator::GenChannel;

pub struct ExecCtx {
    pub stack: Vec<VmValue>,
    pub frames: Vec<CallFrame>,
    pub globals: GlobalStore,
    pub heap: Heap,
    pub try_handlers: Vec<TryHandler>,

    pub modules: FxHashMap<String, VmValue>,

    pub precompiled: Rc<FxHashMap<String, Rc<FunctionProto>>>,

    pub loader: Option<Rc<dyn ModuleLoader>>,
    pub trace: bool,
    pub open_upvalues: Vec<(usize, VmUpvalue)>,
    pub pending_constructors: Vec<(usize, VmValue)>,
    pub pending_setters: Vec<(usize, VmValue)>,
    pub vm_suspend: Option<VmSuspend>,
    pub gen_channel: Option<Rc<GenChannel>>,
    pub deferred_tasks: FxHashMap<usize, Rc<LazyTask>>,
    pub module_exports: FxHashMap<usize, VmValue>,
    pub opcode_counts: Option<Rc<Vec<std::sync::atomic::AtomicU64>>>,
    pub profile_counters: Option<Arc<ProfileCounters>>,

    pub proto_ic_caches: FxHashMap<usize, Rc<RefCell<Vec<varn_types::chunk::PolyICSlot>>>>,
    pub proto_feedback:
        FxHashMap<usize, Rc<RefCell<varn_types::chunk::FeedbackVector>>>,
    pub proto_constants: FxHashMap<usize, Rc<Vec<VmValue>>>,
    pub no_jit: bool,
}

impl ExecCtx {
    pub fn new(mut globals: GlobalStore) -> Self {
        varn_runtime::init_heap();
        let mut heap = Heap::new();

        let fresh = globals.values.is_empty();
        if fresh {
            globals = GlobalStore::with_native_layout(&mut heap);
        }

        let mut ctx = Self {
            stack: Vec::with_capacity(16384),
            frames: Vec::with_capacity(512),
            globals,
            heap,
            try_handlers: Vec::new(),
            modules: FxHashMap::default(),
            precompiled: Rc::new(FxHashMap::default()),
            loader: None,
            trace: false,
            open_upvalues: Vec::new(),
            pending_constructors: Vec::new(),
            pending_setters: Vec::new(),
            vm_suspend: None,
            gen_channel: None,
            deferred_tasks: FxHashMap::default(),
            module_exports: FxHashMap::default(),
            opcode_counts: None,
            profile_counters: None,
            proto_ic_caches: FxHashMap::default(),
            proto_feedback: FxHashMap::default(),
            proto_constants: FxHashMap::default(),
            no_jit: false,
        };

        if fresh {
            ctx.init_intrinsics();
        }
        ctx
    }

    fn init_intrinsics(&mut self) {
        let names = [
            "Array",
            "str",
            "int",
            "float",
            "decimal",
            "bool",
            "char",
            "Map",
            "Set",
            "Range",
            "Error",
            "TypeError",
            "RangeError",
        ];
        for name in names {
            if let Some(nv) = self.globals.get_by_name(name) {
                if let Some(obj) = self.heap.get(nv.as_heap_idx()) {
                    match obj {
                        crate::heap::HeapObj::Class(cls) => {
                            let cls = cls.clone();
                            self.heap.set_intrinsic_class(name, cls);
                        }
                        crate::heap::HeapObj::NativeFn(_, f) => {
                            let f = *f;
                            if let Ok(class_nv) = (f)(self as &mut dyn NativeCtx, &[]) {
                                if let Some(crate::heap::HeapObj::Class(cls)) =
                                    self.heap.get(class_nv.as_heap_idx())
                                {
                                    let cls = cls.clone();
                                    self.heap.set_intrinsic_class(name, cls);
                                    self.globals.set_by_name(name, class_nv);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn fork_for_task(&self) -> Self {
        Self {
            stack: Vec::with_capacity(16384),
            frames: Vec::with_capacity(512),
            globals: self.globals.clone(),
            heap: self.heap.clone(),
            try_handlers: Vec::new(),
            modules: self.modules.clone(),
            precompiled: Rc::clone(&self.precompiled),
            loader: self.loader.clone(),
            trace: self.trace,
            open_upvalues: Vec::new(),
            pending_constructors: Vec::new(),
            pending_setters: Vec::new(),
            vm_suspend: None,
            gen_channel: None,
            deferred_tasks: FxHashMap::default(),
            module_exports: FxHashMap::default(),
            opcode_counts: None,
            profile_counters: None,
            proto_ic_caches: FxHashMap::default(),
            proto_feedback: FxHashMap::default(),
            proto_constants: FxHashMap::default(),
            no_jit: self.no_jit,
        }
    }

    pub fn wait_task_handle(task: varn_types::AsyncTask) -> Result<Value, String> {
        match task.peek_state() {
            varn_types::task::TaskState::Resolved(v) => return Ok(v),
            varn_types::task::TaskState::Rejected(v) => {
                return Err(format!("{v}"));
            }
            varn_types::task::TaskState::Pending => {}
        }

        let (tx, rx) = std::sync::mpsc::channel();
        task.on_settle(move |result| {
            let _ = tx.send(result);
        });

        match rx.recv() {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(v)) => Err(format!("{v}")),
            Err(_) => Err("task dropped".to_owned()),
        }
    }

    pub fn trace_event(
        &self,
        label: &str,
        frame_idx: usize,
        closure: &VmClosure,
        op_ip: usize,
        op: Option<OpCode>,
    ) {
        if !self.trace {
            return;
        }

        let fn_name = closure.proto.name.as_deref().unwrap_or("<anon>");
        let line = closure.proto.chunk.lines.get_line(op_ip);
        eprintln!(
            "[vm:{label}] fn={fn_name} file={} frame={} ip={} line={} stack={} tries={} op={:?}",
            closure.proto.chunk.source_file,
            frame_idx,
            op_ip,
            line,
            self.stack.len(),
            self.try_handlers.len(),
            op,
        );
    }

    pub fn run_lazy_task_sync(
        &mut self,
        task: &varn_types::value::LazyTask,
    ) -> varn_types::AsyncTask {
        let mut fork = self.fork_for_task();
        let constants: Vec<VmValue> = task
            .closure
            .resolved_constants
            .iter()
            .map(|v| fork.heap.intern(v.clone()))
            .collect();
        let upvalues = task
            .closure
            .upvalues
            .iter()
            .map(|uv| {
                let val = uv.inner.borrow_mut().value.clone();
                VmUpvalue::closed(fork.heap.intern(val))
            })
            .collect();
        let cache_count = task.closure.proto.cache_count;
        let new_ic_cache = Rc::new(RefCell::new(
            (0..cache_count)
                .map(|_| varn_types::chunk::PolyICSlot::new())
                .collect(),
        ));
        let new_feedback = Rc::new(RefCell::new(varn_types::chunk::FeedbackVector::new(
            cache_count,
        )));
        let closure = Rc::new(VmClosure::with_upvalues(
            Rc::clone(&task.closure.proto),
            upvalues,
            Rc::new(constants),
            new_ic_cache,
            new_feedback,
        ));
        let stack_values: Vec<VmValue> = task
            .args
            .iter()
            .cloned()
            .map(|value| fork.heap.intern(value))
            .collect();
        fork.stack = stack_values;
        let required = task.closure.proto.register_count as usize;
        if fork.stack.len() < required {
            fork.stack.resize(required, VmValue::null());
        }
        let mut frame = CallFrame::new(closure, 0);
        frame.current_class = task.current_class.clone();
        fork.frames.push(frame);

        let output = varn_types::AsyncTask::pending();
        loop {
            match fork.run() {
                Ok(result) => match fork.vm_suspend.take() {
                    Some(crate::exec::VmSuspend::Await { value, dest_reg }) => {
                        let resolved = match value {
                            varn_types::Value::Task(lazy) => {
                                let h = fork.run_lazy_task_sync(lazy.as_ref());
                                match h.peek_state() {
                                    varn_types::task::TaskState::Resolved(v) => v,
                                    _ => varn_types::Value::Null,
                                }
                            }
                            varn_types::Value::TaskHandle(handle) => match handle.peek_state() {
                                varn_types::task::TaskState::Resolved(v) => v,
                                _ => varn_types::Value::Null,
                            },
                            other => other,
                        };
                        let resolved_nv = fork.heap.intern(resolved);
                        if let Some(frame) = fork.frames.last() {
                            let base = frame.base;
                            let slot = base + dest_reg as usize;
                            if slot < fork.stack.len() {
                                fork.stack[slot] = resolved_nv;
                            } else {
                                fork.stack.resize(slot + 1, VmValue::null());
                                fork.stack[slot] = resolved_nv;
                            }
                        }
                    }
                    None => {
                        output.resolve(fork.heap.extract(result));
                        break;
                    }
                    Some(_) => {
                        output.resolve(fork.heap.extract(result));
                        break;
                    }
                },
                Err(err) => {
                    output.reject_msg(err.message);
                    break;
                }
            }
        }

        self.heap = fork.heap;
        self.globals = fork.globals;
        self.modules = fork.modules;

        output
    }

    pub fn exec_run_deferred(&mut self, handle: &varn_types::AsyncTask) {
        let key = handle.ptr_key();
        if let Some(lazy) = self.deferred_tasks.remove(&key) {
            let resolved = self.run_lazy_task_sync(lazy.as_ref());
            match resolved.peek_state() {
                varn_types::task::TaskState::Resolved(v) => handle.resolve(v),
                varn_types::task::TaskState::Rejected(v) => handle.reject(v),
                varn_types::task::TaskState::Pending => {}
            }
        }
    }

    pub fn push(&mut self, v: VmValue) {
        self.stack.push(v);
    }

    pub fn pop(&mut self) -> VmResult<VmValue> {
        self.stack
            .pop()
            .ok_or_else(|| RuntimeError::new("stack underflow"))
    }

    #[inline(always)]
    pub fn record_ic_hit(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_hit();
        }
    }

    #[inline(always)]
    pub fn record_ic_miss(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_miss();
        }
    }

    #[inline(always)]
    pub fn record_ic_hit_getprop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_hit_getprop();
        }
    }

    #[inline(always)]
    pub fn record_ic_miss_getprop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_miss_getprop();
        }
    }

    #[inline(always)]
    pub fn record_ic_hit_setprop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_hit_setprop();
        }
    }

    #[inline(always)]
    pub fn record_ic_miss_setprop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_miss_setprop();
        }
    }

    #[inline(always)]
    pub fn record_ic_hit_callmethod(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_hit_callmethod();
        }
    }

    #[inline(always)]
    pub fn record_ic_miss_callmethod(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_miss_callmethod();
        }
    }

    #[inline(always)]
    pub fn record_call_vm_fast(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_call_vm_fast();
        }
    }

    #[inline(always)]
    pub fn record_call_slow(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_call_slow();
        }
    }

    #[inline(always)]
    pub fn record_call_native(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_call_native();
        }
    }

    #[inline(always)]
    pub fn record_reg_load(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_reg_load();
        }
    }

    #[inline(always)]
    pub fn record_reg_store(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_reg_store();
        }
    }

    #[inline(always)]
    pub fn record_frame_push(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_frame_push();
        }
    }

    #[inline(always)]
    pub fn record_frame_pop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_frame_pop();
        }
    }

    pub fn trigger_gc(&mut self) {
        let mut roots: Vec<u32> = Vec::with_capacity(256);
        for v in &self.stack {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for v in &self.globals.values {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }

        for (_k, v) in &self.modules {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for frame in &self.frames {
            for c in frame.closure.constants.iter() {
                if c.is_heap() {
                    roots.push(c.as_heap_idx());
                }
            }
        }
        let _ = self.heap.collect(&roots);
    }

    #[inline(always)]
    pub fn prepare_call(&mut self, callee_nv: VmValue, arg_count: usize) -> VmResult<PreparedCall> {
        if let Some((prepared, needs_receiver)) =
            calls::try_prepare_call_fast(callee_nv, arg_count, &self.stack, &self.heap)
        {
            if needs_receiver {
                if callee_nv.is_heap() {
                    if let Some(crate::heap::HeapObj::BoundMethod(bm)) =
                        self.heap.get(callee_nv.as_heap_idx())
                    {
                        let recv_nv = self.heap.intern(bm.receiver.clone());

                         match prepared {
                            calls::PreparedCall::Frame(ref frame) => {
                                if frame.base >= self.stack.len() {
                                    self.stack.push(recv_nv);
                                } else {
                                    self.stack[frame.base] = recv_nv;
                                }
                            }
                            calls::PreparedCall::NativeImmediate(_, _) | calls::PreparedCall::RawNativeImmediate(_, _) => {
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
        calls::prepare_call(
            callee_nv,
            arg_count,
            &mut self.stack,
            &mut self.heap,
            &mut self.globals,
        )
    }

    pub fn push_frame(&mut self, closure: Rc<VmClosure>) -> crate::error::VmResult<()> {
        if self.frames.len() >= 10000 {
            return Err(crate::error::RuntimeError::new("stack overflow: call depth exceeded 10000"));
        }
        let base = self.stack.len();
        let required = base + closure.proto.register_count as usize;
        if self.stack.len() < required {
            self.stack.resize(required, VmValue::null());
        }
        self.record_frame_push();
        self.frames.push(CallFrame::new(closure, base));
        Ok(())
    }

    pub fn push_frame_at(&mut self, closure: Rc<VmClosure>, base: usize) {
        let required = base + closure.proto.register_count as usize;
        if self.stack.len() < required {
            self.stack.resize(required, VmValue::null());
        }
        self.record_frame_push();
        self.frames.push(CallFrame::new(closure, base));
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

    pub fn load_module(&mut self, specifier: &str) -> VmResult<VmValue> {
        use crate::exec::modules;

        let source_file = self
            .frames
            .last()
            .map(|f| f.closure.proto.chunk.source_file.clone())
            .unwrap_or_else(|| "".to_owned().into());
        let resolved = modules::resolve_specifier_from_path(specifier, &source_file.to_string())?;
        if let Some(&cached) = self.modules.get(&resolved.as_str()) {
            return Ok(cached);
        }

        if let ModuleId::Stdlib(ref name) = resolved {
            if let Some(nv) = varn_builtins::build_module(name, &mut self.heap) {
                self.modules.insert(resolved.as_str().to_string(), nv);
                return Ok(nv);
            }
        }

        if let Some(proto) = self.precompiled.get(&resolved.as_str()).cloned() {
            if let Some(existing_idx) = self.frames.iter().position(|f| {
                f.closure
                    .proto
                    .chunk
                    .module_id
                    .as_ref()
                    .map(|id| id.as_str() == resolved.as_str())
                    .unwrap_or(false)
            }) {
                let res = self.run_until(existing_idx)?;
                if self.vm_suspend.is_some() {
                    return Ok(VmValue::null());
                }
                let final_val = self.modules.get(&resolved.as_str()).copied().unwrap_or(res);
                self.modules.insert(resolved.as_str(), final_val);
                return Ok(final_val);
            }

            let closure = crate::exec::calls::build_closure(proto, &mut self.heap);
            self.push_frame(closure)?;
            let res = self.run_until(self.frames.len() - 1)?;
            if self.vm_suspend.is_some() {
                return Ok(VmValue::null());
            }
            let final_val = self.modules.get(&resolved.as_str()).copied().unwrap_or(res);
            self.modules.insert(resolved.as_str(), final_val);
            return Ok(final_val);
        }

        if let Some(loader) = &self.loader {
            if let Ok(Some(proto)) = loader.load(&resolved) {
                if let Some(existing_idx) = self.frames.iter().position(|f| {
                    f.closure
                        .proto
                        .chunk
                        .module_id
                        .as_ref()
                        .map(|id| id.as_str() == resolved.as_str())
                        .unwrap_or(false)
                }) {
                    let res = self.run_until(existing_idx)?;
                    if self.vm_suspend.is_some() {
                        return Ok(VmValue::null());
                    }
                    let final_val = self.modules.get(&resolved.as_str()).copied().unwrap_or(res);
                    self.modules.insert(resolved.as_str(), final_val);
                    return Ok(final_val);
                }

                let closure = crate::exec::calls::build_closure(proto, &mut self.heap);
                self.push_frame(closure)?;
                let res = self.run_until(self.frames.len() - 1)?;
                if self.vm_suspend.is_some() {
                    return Ok(VmValue::null());
                }
                let final_val = self.modules.get(&resolved.as_str()).copied().unwrap_or(res);
                self.modules.insert(resolved.as_str(), final_val);
                return Ok(final_val);
            }
        }

        Err(RuntimeError::new(format!(
            "module not found: {}",
            specifier
        )))
    }
}

// --- JIT Runtime Helper Callouts ---

pub extern "C" fn jit_load_const(closure: *const crate::frame::VmClosure, idx: usize) -> VmValue {
    unsafe {
        let closure_ref = &*closure;
        closure_ref.constants[idx]
    }
}

pub extern "C" fn jit_load_global_idx(ctx: *mut ExecCtx, idx: usize) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.globals.get_by_index(idx).unwrap_or(VmValue::null())
    }
}

pub extern "C" fn jit_store_global_idx(ctx: *mut ExecCtx, idx: usize, val: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.globals.set_by_index(idx, val);
    }
}

pub extern "C" fn jit_define_global_idx(ctx: *mut ExecCtx, idx: usize, val: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.globals.set_by_index(idx, val);
    }
}

