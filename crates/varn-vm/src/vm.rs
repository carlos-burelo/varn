use crate::closure::{VmClosure, VmUpvalue, VmUpvalueInner};
use crate::exec;
use crate::globals::GlobalStore;
use crate::heap::Heap;
use crate::loader::ModuleLoader;
use crate::profile::{HotspotCounters, ProfileCounters, VmProfile};
use crate::value::VmValue;
use exec::calls;
use exec::ExecCtx;

use std::cell::RefCell;
use std::cmp::Reverse;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use varn_core::{ModuleId, OpCode};
use varn_types::chunk::FunctionProto;
use varn_types::Closure;

pub struct Vm {
    pub ctx: ExecCtx,
}

impl Vm {
    pub fn new(
        precompiled: Rc<rustc_hash::FxHashMap<ModuleId, Rc<varn_types::FunctionProto>>>,
        settings: crate::settings::ExecSettings,
    ) -> Self {
        let mut ctx = ExecCtx::new(GlobalStore::new(), settings);
        ctx.precompiled = precompiled;
        Self { ctx }
    }

    pub fn with_loader(mut self, loader: std::sync::Arc<dyn ModuleLoader + Send + Sync>) -> Self {
        self.ctx.loader = Some(loader);
        self
    }

    pub fn from_snapshot(
        globals: GlobalStore,
        heap: Heap,
        precompiled: Rc<rustc_hash::FxHashMap<ModuleId, Rc<varn_types::FunctionProto>>>,
        modules: rustc_hash::FxHashMap<ModuleId, VmValue>,
        settings: crate::settings::ExecSettings,
    ) -> Self {
        let mut ctx = ExecCtx::new(globals, settings);
        ctx.heap = heap.deep_clone();
        ctx.precompiled = precompiled;
        ctx.modules = modules;
        Self { ctx }
    }

    pub fn run(&mut self, closure: Rc<Closure>) -> Result<VmValue, crate::error::RuntimeError> {
        let constants = calls::resolve_constants(&closure.proto, &mut self.ctx.heap);
        let upvalues = closure
            .upvalues
            .iter()
            .map(|uv| {
                let inner = uv.inner.borrow();
                VmUpvalue {
                    inner: Rc::new(RefCell::new(VmUpvalueInner {
                        value: self.ctx.heap.intern(inner.value.clone()),
                        stack_slot: inner.location,
                    })),
                }
            })
            .collect();

        let nan_closure = Rc::new(VmClosure::with_upvalues(
            closure.proto.clone(),
            upvalues,
            Rc::new(constants),
            self.ctx.settings,
        ));

        if self.ctx.frames.is_empty() {
            self.ctx.push_frame(nan_closure)?;
        }

        self.ctx.run()
    }

    pub fn snapshot(&self) -> (GlobalStore, Heap, rustc_hash::FxHashMap<ModuleId, VmValue>) {
        let native_modules = self
            .ctx
            .modules
            .iter()
            .filter(|(id, _)| {
                matches!(
                    id,
                    ModuleId::Std(_) | ModuleId::Core(_) | ModuleId::Runtime(_)
                )
            })
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        (
            self.ctx.globals.clone(),
            self.ctx.heap.clone(),
            native_modules,
        )
    }

    pub fn enable_opcode_profiling(&mut self) {
        let mut v = Vec::with_capacity(512);
        for _ in 0..512 {
            v.push(AtomicU64::new(0));
        }
        self.ctx.opcode_counts = Some(Rc::new(v));
    }

    pub fn enable_profiling(&mut self) {
        let counters = ProfileCounters::new();
        // The frame stack counts its own pushes and pops, so it needs the same
        // handle — see `crate::frame_stack`.
        self.ctx.frames.set_counters(Some(counters.clone()));
        self.ctx.profile_counters = Some(counters);
    }

    pub fn enable_hotspot_profiling(&mut self) {
        let counters = HotspotCounters::new();
        self.ctx.hotspot_counters = Some(counters.clone());
        self.ctx.heap.hotspot = Some(counters);
    }

    pub fn take_hotspots(&mut self) -> Option<HotspotCounters> {
        self.ctx.heap.hotspot = None;
        self.ctx
            .hotspot_counters
            .take()
            .map(|rc| match Rc::try_unwrap(rc) {
                Ok(cell) => cell.into_inner(),
                Err(rc) => rc.borrow().clone(),
            })
    }

    pub fn take_profile(&mut self) -> Option<VmProfile> {
        self.ctx.profile_counters.take().map(|arc| {
            let profile = VmProfile::from_counters(&arc);
            VmProfile {
                heap_allocs: self.ctx.heap.alloc_count,
                gc_collections: self.ctx.heap.gc_collections,
                gc_freed: self.ctx.heap.gc_total_freed,
                heap_live: self.ctx.heap.live_count() as u64,
                heap_total: self.ctx.heap.objects_len() as u64,
                nursery_allocs: self.ctx.heap.nursery.alloc_count,
                minor_gc_count: self.ctx.heap.nursery.minor_gc_count,
                minor_gc_promoted: self.ctx.heap.nursery.minor_gc_promoted,
                ..profile
            }
        })
    }

    pub fn collect_gc(&mut self) -> usize {
        let mut roots: Vec<u32> = self
            .ctx
            .stack
            .iter()
            .chain(self.ctx.globals.values.iter())
            .filter(|v| v.is_heap())
            .map(|v| v.as_heap_idx())
            .collect();
        for v in self.ctx.modules.values() {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for v in self.ctx.module_exports.values() {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        self.ctx.heap.collect(&roots)
    }

    pub fn take_opcode_counts(&mut self) -> Vec<(OpCode, u64)> {
        let counts = match self.ctx.opcode_counts.take() {
            Some(c) => c,
            None => return Vec::new(),
        };
        let mut result = Vec::new();
        for (i, c) in counts.iter().enumerate() {
            let val = c.load(Ordering::Relaxed);
            if val > 0 {
                if let Some(op) = OpCode::from_u16(i as u16) {
                    result.push((op, val));
                }
            }
        }
        result.sort_by_key(|(_, c)| Reverse(*c));
        result
    }

    /// Bind `proto`'s global accesses to this VM's slot indices.
    ///
    /// Only the ENTRY proto needs this from outside: every other proto — every
    /// module, in every VM, from `precompiled` or from a `ModuleLoader` — is
    /// resolved by `ExecCtx::eval_module_proto` as it enters. The entry proto
    /// is the one that never passes through there.
    ///
    /// Deliberately NOT folded into [`Vm::run`]: the bench harness builds a
    /// fresh VM and calls `run` inside the timed region, so resolving there
    /// would charge every measured iteration for work that belongs to setup.
    pub fn resolve_globals(&mut self, proto: &mut FunctionProto) {
        crate::globals::resolve_in_proto(proto, &mut self.ctx.globals);
    }
}

pub fn prefill_native_modules(vm: &mut Vm) {
    for raw_id in varn_builtins::all_native_module_ids() {
        if !raw_id.contains(':') {
            continue;
        }

        let resolved = varn_core::ModuleId::from_canonical_str(&raw_id);
        if vm.ctx.modules.contains_key(&resolved) {
            continue;
        }

        if let Some(spec) = varn_builtins::spec_for(&raw_id) {
            if spec.pure {
                continue;
            }
        }

        if let Some(nv) = varn_builtins::build_module(&raw_id, &mut vm.ctx.heap) {
            if let Ok(converted) = vm.ctx.convert_to_module_obj(resolved.clone(), nv) {
                vm.ctx.modules.insert(resolved, converted);
            }
        }
    }

    freeze_pure_modules(vm);
}

fn freeze_pure_modules(vm: &mut Vm) {
    let pure_ids: Vec<&'static str> = varn_builtins::MODULE_REGISTRY
        .iter()
        .filter(|s| s.pure)
        .map(|s| s.id)
        .collect();

    if pure_ids.is_empty() {
        return;
    }

    let mut scratch = Vm::new(vm.ctx.precompiled.clone(), vm.ctx.settings);
    if let Some(loader) = &vm.ctx.loader {
        scratch.ctx.loader = Some(loader.clone());
    }

    for (id, &val) in &vm.ctx.modules {
        if matches!(id, varn_core::ModuleId::Runtime(_)) {
            if let Some(frozen_arc) =
                crate::exec::ctx_modules::freeze_module(val, id.clone(), &vm.ctx.heap)
            {
                let fv = scratch.ctx.heap.alloc_frozen_module(frozen_arc);
                scratch.ctx.modules.insert(id.clone(), fv);
                scratch.ctx.linker.set_done(id.clone(), fv);
            }
        }
    }

    for id_str in pure_ids {
        let resolved = varn_core::ModuleId::from_canonical_str(id_str);

        if matches!(resolved, varn_core::ModuleId::Core(_)) {
            continue;
        }

        if vm.ctx.modules.contains_key(&resolved) {
            continue;
        }

        let load_result = scratch.ctx.load_module(id_str);
        let module_val = match load_result {
            Ok(v) => v,
            Err(_) => continue,
        };

        let Some(frozen) = crate::exec::ctx_modules::freeze_module(
            module_val,
            resolved.clone(),
            &scratch.ctx.heap,
        ) else {
            continue;
        };

        let frozen_val = vm.ctx.heap.alloc_frozen_module(frozen);
        vm.ctx.modules.insert(resolved.clone(), frozen_val);
        vm.ctx.linker.set_done(resolved, frozen_val);
    }
}

