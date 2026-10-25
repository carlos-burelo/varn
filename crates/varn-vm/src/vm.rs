use crate::closure::VmClosure;
use crate::exec;
use crate::globals::GlobalStore;
use crate::loader::ModuleLoader;
use crate::profile::{HotspotCounters, ProfileCounters, VmProfile};
use crate::value::VmValue;
use exec::calls;
use exec::ExecCtx;

use std::cmp::Reverse;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use varn_core::{ModuleId, OpCode};

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

    pub fn run(
        &mut self,
        proto: Rc<varn_types::FunctionProto>,
    ) -> Result<VmValue, crate::error::RuntimeError> {
        let constants = calls::resolve_constants(&proto, &mut self.ctx.heap);
        if self.ctx.frames.is_empty() {
            let mut entry = VmClosure::with_upvalues(
                proto.clone(),
                Vec::new(),
                Rc::new(constants),
                self.ctx.settings,
            );
            entry.module_base = self
                .ctx
                .globals_mut()
                .reserve_module_region(proto.global_count, true);
            self.ctx.main_module_base.get_or_insert(entry.module_base);
            self.ctx.push_frame(Rc::new(entry))?;
        }

        let returned = self.ctx.run()?;
        let rejected = self
            .ctx
            .heap
            .unobserved_rejections
            .iter()
            .find(|(cell, _)| cell.rejection_unobserved())
            .map(|(_, text)| text.clone());
        match rejected {
            Some(text) => Err(crate::error::RuntimeError::new(format!(
                "unhandled task rejection: {text}"
            ))),
            None => Ok(returned),
        }
    }

    pub fn call_main_global(&mut self, global: u32) -> Result<(), crate::error::RuntimeError> {
        let base = self
            .ctx
            .main_module_base
            .ok_or_else(|| crate::error::RuntimeError::new("the main module has not run yet"))?;
        let callee = self
            .ctx
            .globals_mut()
            .get_by_index(base as usize + global as usize)
            .ok_or_else(|| {
                crate::error::RuntimeError::new(format!(
                    "module global {global} is outside the main module region"
                ))
            })?;
        let returned = self.ctx.invoke(callee, &[callee])?;
        let lazy = returned
            .is_heap()
            .then(|| self.ctx.heap.get(returned.as_heap()))
            .flatten()
            .and_then(|obj| {
                if let crate::heap::HeapObj::Task(lazy) = obj {
                    Some(Rc::clone(lazy))
                } else {
                    None
                }
            });
        let cell = match lazy {
            Some(lazy) => Some(self.ctx.run_lazy_task_sync(lazy)),
            None => self.ctx.task_cell(returned),
        };
        let Some(cell) = cell else {
            return Ok(());
        };
        match self.ctx.settle_awaited(&cell) {
            Ok(_) => Ok(()),
            Err(thrown) => Err(exec::exceptions::build_thrown_error(
                thrown,
                &self.ctx.heap,
                &[],
            )),
        }
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

        self.ctx.frames.set_counters(Some(counters.clone()));
        self.ctx.profile_counters = Some(counters);
    }

    pub fn enable_hotspot_profiling(&mut self) {
        let counters = HotspotCounters::new();
        self.ctx.hotspot_counters = Some(counters.clone());
        self.ctx.heap.hotspot = Some(counters);
    }

    pub fn abandon_heap_at_exit(&mut self) {
        self.ctx.heap.cells.abandon_objects = true;
    }

    pub fn gc_report(&self) -> crate::gc_report::GcReport {
        self.ctx.heap.gc_report()
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
            let tasks = crate::profile::TASK_STATS.snapshot();
            VmProfile {
                heap_allocs: self.ctx.heap.alloc_count(),
                gc_collections: self.ctx.heap.gc_collections,
                gc_freed: self.ctx.heap.gc_total_freed,
                heap_live: self.ctx.heap.live_count() as u64,
                heap_total: self.ctx.heap.objects_len() as u64,
                nursery_allocs: self.ctx.heap.young.alloc_count(),
                minor_gc_count: self.ctx.heap.young.minor_gc_count,
                minor_gc_promoted: self.ctx.heap.young.minor_gc_promoted,
                task_parks: tasks.parks,
                task_released: tasks.released,
                task_prune_hit: tasks.prune_hit,
                task_prune_miss: tasks.prune_miss,
                task_yields: tasks.yields,
                timer_purged: varn_builtins::runtime::timer::purged(),
                ..profile
            }
        })
    }

    pub fn collect_gc(&mut self) -> usize {
        self.ctx.trigger_gc()
    }

    pub fn take_opcode_counts(&mut self) -> Vec<(OpCode, u64)> {
        let counts = match self.ctx.opcode_counts.take() {
            Some(c) => c,
            None => return Vec::new(),
        };
        let mut result = Vec::new();
        for (i, c) in counts.iter().enumerate() {
            let val = c.load(Ordering::Relaxed);
            if val > 0
                && let Some(op) = OpCode::from_u16(i as u16)
            {
                result.push((op, val));
            }
        }
        result.sort_by_key(|(_, c)| Reverse(*c));
        result
    }
}

pub fn prefill_native_modules(vm: &mut Vm) {
    for raw_id in varn_builtins::all_native_module_ids() {
        if !raw_id.contains(':') {
            continue;
        }

        let resolved = varn_core::ModuleId::from_canonical_str(&raw_id);
        if unsafe { &*vm.ctx.modules.get() }.contains_key(&resolved) {
            continue;
        }

        if let Some(spec) = varn_builtins::spec_for(&raw_id)
            && spec.pure
        {
            continue;
        }

        if let Some(nv) = varn_builtins::build_module(&raw_id, &mut vm.ctx.heap)
            && let Ok(converted) = vm.ctx.convert_to_module_obj(resolved.clone(), nv)
        {
            unsafe { &mut *vm.ctx.modules.get() }.insert(resolved, converted);
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

    for (id, &val) in unsafe { &*vm.ctx.modules.get() }.iter() {
        if matches!(id, varn_core::ModuleId::Runtime(_))
            && let Some(frozen_arc) =
                crate::exec::module_freeze::freeze_module(val, id.clone(), &vm.ctx.heap)
        {
            let fv = scratch.ctx.heap.alloc_frozen_module(frozen_arc);
            unsafe { &mut *scratch.ctx.modules.get() }.insert(id.clone(), fv);
            scratch.ctx.linker.set_done(id.clone(), fv);
        }
    }

    for id_str in pure_ids {
        let resolved = varn_core::ModuleId::from_canonical_str(id_str);

        if matches!(resolved, varn_core::ModuleId::Core(_)) {
            continue;
        }

        if unsafe { &*vm.ctx.modules.get() }.contains_key(&resolved) {
            continue;
        }

        let load_result = scratch.ctx.load_module(id_str);
        let module_val = match load_result {
            Ok(v) => v,
            Err(_) => continue,
        };

        let Some(frozen) = crate::exec::module_freeze::freeze_module(
            module_val,
            resolved.clone(),
            &scratch.ctx.heap,
        ) else {
            continue;
        };

        let frozen_val = vm.ctx.heap.alloc_frozen_module(frozen);
        unsafe { &mut *vm.ctx.modules.get() }.insert(resolved.clone(), frozen_val);
        vm.ctx.linker.set_done(resolved, frozen_val);
    }
}
