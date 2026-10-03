use crate::closure::VmUpvalue;
use crate::frame::TryHandler;
use crate::globals::GlobalStore;
use crate::heap::Heap;
use crate::loader::ModuleLoader;
use crate::profile::{HotspotCounters, ProfileCounters};
use crate::value::VmValue;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use varn_core::ModuleId;
use varn_types::{FunctionProto, NativeCtx};

use crate::linker::Linker;

use super::VmSuspend;

type SharedModules = Rc<std::cell::UnsafeCell<FxHashMap<ModuleId, VmValue>>>;
type SharedProtoPools =
    Rc<std::cell::UnsafeCell<FxHashMap<usize, (Rc<FunctionProto>, Rc<Vec<VmValue>>)>>>;
type SharedStaticClosures =
    Rc<std::cell::UnsafeCell<FxHashMap<usize, (Rc<FunctionProto>, VmValue)>>>;
type SharedMetadata = Rc<std::cell::UnsafeCell<FxHashMap<String, FxHashMap<String, VmValue>>>>;
type SharedHashableKeys = Rc<std::cell::UnsafeCell<FxHashMap<(u32, i64), Vec<VmValue>>>>;

#[repr(C)]
pub struct ExecCtx {
    pub stack: crate::frame_store::FrameStore,
    pub frames: crate::frame_stack::FrameStack,
    /// Module-global region, shared across every task fork like the heap:
    /// sibling tasks observe each other's defines in drive order (structured
    /// scope). Same single-threaded `Rc<UnsafeCell>` contract as the heap;
    /// access only through [`Self::globals_ref`] / [`Self::globals_mut`],
    /// never held across nested VM entry.
    pub globals: Rc<std::cell::UnsafeCell<GlobalStore>>,
    pub heap: Heap,
    pub try_handlers: Vec<TryHandler>,
    pub modules: SharedModules,
    pub precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    pub loader: Option<std::sync::Arc<dyn ModuleLoader + Send + Sync>>,
    pub settings: crate::settings::ExecSettings,
    pub open_upvalues: Vec<(crate::frame_store::SlotAddr, VmUpvalue)>,
    pub pending_constructors: Vec<(usize, VmValue)>,
    pub pending_setters: Vec<(usize, VmValue)>,
    pub vm_suspend: Option<VmSuspend>,
    pub module_exports: FxHashMap<usize, VmValue>,
    pub opcode_counts: Option<Rc<Vec<std::sync::atomic::AtomicU64>>>,
    pub profile_counters: Option<Arc<ProfileCounters>>,
    pub hotspot_counters: Option<Rc<RefCell<HotspotCounters>>>,
    /// Both of these are keyed by `Rc::as_ptr(&proto)`, and each entry holds a
    /// strong ref to the proto that address belongs to. That ref is not
    /// decoration: without it the `Rc` can be dropped while the entry lives on,
    /// the allocator can hand the same address to a DIFFERENT proto, and the
    /// cache then answers with another function's constant pool — a silent
    /// miscompile ("a" + `<object>` + "b" where a literal should be). Holding
    /// the proto makes the address ours for as long as it is a key.
    pub proto_constants: SharedProtoPools,
    pub static_closures: SharedStaticClosures,
    pub linker: Linker,
    pub jit_jmp_buf: *mut JmpBuf,
    /// The OUTERMOST clif frame's jump buffer. `jit_jmp_buf` is per-frame so
    /// a throw unwinds one clif frame at a time, but a suspension cannot do
    /// that — an intermediate clif frame has no way to park in the middle of
    /// a native function — so `jit_await`/`jit_yield` jump here instead.
    pub jit_suspend_buf: *mut JmpBuf,
    pub jit_panic_exception_handler: Option<crate::frame::TryHandler>,
    pub jit_panic_exception_error: Option<VmValue>,
    pub jit_panic_exception_err_obj: Option<crate::error::RuntimeError>,
    pub jit_panic_suspend_resume_ip: Option<usize>,
    pub jit_native_result: VmValue,
    /// An ON-STACK REPLACEMENT request raised by `OpCode::Loop` when a proto's
    /// back edges crossed the threshold, holding the loop-header ip to resume
    /// at. The opcode cannot service it itself — entering compiled code means
    /// leaving the dispatch loop — so it parks the ip here and returns
    /// `ContinueFrame`; the frame loop takes it on the way round.
    pub osr_request: Option<usize>,
    /// Process-global resource table, shared across every task fork like the
    /// heap: resource ids are unique per process, so a fork-local copy would
    /// alias live ids and drop handles created inside tasks. Same
    /// single-threaded `Rc<UnsafeCell>` contract as [`Heap`](crate::heap::Heap).
    pub resources: Rc<std::cell::UnsafeCell<varn_types::ResourceStore>>,
    /// Nested contexts (sync-generator bodies) share the heap but own a
    /// private stack the outer context's GC roots cannot see in the other
    /// direction: a collection triggered from *inside* the nested context
    /// would never root the suspended outer stack. Such contexts must never
    /// initiate a collection — allocation overflows to the old gen when the
    /// nursery is full, and the owning context collects (tracing the
    /// generator's saved state via `GeneratorDriver::trace_vm_values_mut`)
    /// at its next safepoint. Keep this LAST: `ExecCtx` is `#[repr(C)]` and
    /// JIT code addresses leading fields by raw offset.
    pub gc_inhibited: bool,
    pub capabilities: Rc<varn_types::capabilities::CapabilitySet>,
    pub metadata: SharedMetadata,
    /// Map/Set key representatives of `Hashable & Equatable` instances, by
    /// `(class id, hash())` (see `hashable_keys.rs`). GC roots.
    pub(crate) hashable_keys: SharedHashableKeys,
    pub gc_root_scratch: Vec<VmValue>,
    /// Staging para el protocolo lento de llamadas (P2): la ventana
    /// callee+args como `Vec<VmValue>` contiguo, reutilizado entre llamadas.
    /// `prepare_call` la consume de forma síncrona (adopta al frame o la
    /// drena), así que nunca hay staging anidado vivo a la vez.
    pub stage: Vec<VmValue>,
    /// Cola del scheduler cooperativo: forks listos y parqueados que este
    /// contexto drivea. Va al final junto a `stage`: ningún código JIT
    /// direcciona estos campos por offset crudo.
    pub(crate) task_queue: std::cell::OnceCell<super::scheduler::TaskQueue>,
}

impl ExecCtx {
    pub(crate) fn new(mut globals: GlobalStore, settings: crate::settings::ExecSettings) -> Self {
        let mut heap = Heap::new();

        let fresh = globals.values.is_empty();
        if fresh {
            globals = GlobalStore::with_native_layout(&mut heap);
        }

        let mut ctx = Self {
            stack: crate::frame_store::FrameStore::new(),
            frames: crate::frame_stack::FrameStack::with_capacity(512),
            globals: Rc::new(std::cell::UnsafeCell::new(globals)),
            heap,
            try_handlers: Vec::new(),
            modules: Rc::new(std::cell::UnsafeCell::new(FxHashMap::default())),
            precompiled: Rc::new(FxHashMap::default()),
            loader: None,
            settings,
            open_upvalues: Vec::new(),
            pending_constructors: Vec::new(),
            pending_setters: Vec::new(),
            vm_suspend: None,
            module_exports: FxHashMap::default(),
            opcode_counts: None,
            profile_counters: None,
            hotspot_counters: None,
            proto_constants: Rc::new(std::cell::UnsafeCell::new(FxHashMap::default())),
            static_closures: Rc::new(std::cell::UnsafeCell::new(FxHashMap::default())),
            linker: Linker::new(),
            jit_jmp_buf: std::ptr::null_mut(),
            jit_suspend_buf: std::ptr::null_mut(),
            jit_panic_exception_handler: None,
            jit_panic_exception_error: None,
            jit_panic_exception_err_obj: None,
            jit_panic_suspend_resume_ip: None,
            jit_native_result: VmValue::null(),
            osr_request: None,
            resources: Rc::new(std::cell::UnsafeCell::new(varn_types::ResourceStore::new())),
            gc_inhibited: false,
            capabilities: Rc::new(varn_types::capabilities::CapabilitySet::allow_all()),
            metadata: Rc::new(std::cell::UnsafeCell::new(FxHashMap::default())),
            hashable_keys: Rc::new(std::cell::UnsafeCell::new(FxHashMap::default())),
            gc_root_scratch: Vec::with_capacity(1024),
            stage: Vec::with_capacity(32),
            task_queue: std::cell::OnceCell::new(),
        };

        if fresh {
            ctx.init_intrinsics();
            ctx.preload_strings();
        }

        ctx.validate_jit_safepoint_offsets();
        ctx
    }

    /// The JIT back-edge safepoint reads the nursery fill level through raw
    /// offsets (ExecCtx.heap -> RcBox -> HeapInner.nursery.objects.len), and
    /// `emit_nursery_alloc` (`crates/varn-jit/src/clif/nursery.rs`) writes
    /// through two more: the `forwarding` Vec and `Nursery::alloc_count`.
    /// These offsets all bake in Rc/Vec internal layout; verify the whole
    /// chain against the live heap so a std layout change fails loudly at
    /// startup instead of corrupting memory at runtime.
    fn validate_jit_safepoint_offsets(&self) {
        unsafe {
            let base = self as *const ExecCtx as *const u8;
            let rcbox = *(base.add(std::mem::offset_of!(ExecCtx, heap)) as *const *const u8);
            assert_eq!(
                rcbox,
                self.heap.rcbox_ptr_for_validation(),
                "JIT safepoint: ExecCtx.heap does not point at the expected RcBox"
            );
            let len = *(rcbox.add(Heap::nursery_len_byte_offset_from_rcbox()) as *const usize);
            assert_eq!(
                len,
                self.heap.nursery.len(),
                "JIT safepoint: nursery length offset chain is stale"
            );

            // `emit_nursery_alloc` bumps `forwarding`'s length word alongside
            // `objects`'; the two must always agree (`try_alloc` pushes to
            // both together, and `collect` clears both together), so reading
            // it back through the offset chain and comparing against
            // `objects.len()` catches a stale/wrong offset the same way the
            // check above does.
            let fwd_len_off =
                Heap::nursery_fwd_vec_byte_offset_from_rcbox() + 2 * std::mem::size_of::<usize>();
            let fwd_len = *(rcbox.add(fwd_len_off) as *const usize);
            assert_eq!(
                fwd_len,
                self.heap.nursery.len(),
                "JIT allocation: nursery forwarding-vec offset chain is stale"
            );

            // `Nursery::alloc_count` is a plain field, so its raw-read value
            // must match exactly, not just be consistent with another read.
            let alloc_count =
                *(rcbox.add(Heap::nursery_alloc_count_byte_offset_from_rcbox()) as *const u64);
            assert_eq!(
                alloc_count, self.heap.nursery.alloc_count,
                "JIT allocation: nursery alloc_count offset chain is stale"
            );
        }
    }

    fn preload_strings(&mut self) {
        const COMMON_STRINGS: &[&str] = &[
            "PASSED",
            "FAILED",
            "error",
            "message",
            "value",
            "result",
            "length",
            "name",
            "type",
            "ok",
            "err",
            "true",
            "false",
            "toString",
            "valueOf",
            "constructor",
        ];
        for s in COMMON_STRINGS {
            self.heap.alloc_str_interned(s);
        }
    }

    fn init_intrinsics(&mut self) {
        // Intrinsic classes the VM registers for property/method fallback
        // dispatch. Names are sourced from the canonical `RuntimeKind` names
        // (no raw literals). This set is broader than the op-id core classes:
        // it includes the `Error` hierarchy but not `Symbol`/`bigint`.
        let names = [
            varn_core::RuntimeKind::Array.name(),
            varn_core::RuntimeKind::Str.name(),
            varn_core::RuntimeKind::Int.name(),
            varn_core::RuntimeKind::Float.name(),
            varn_core::RuntimeKind::Decimal.name(),
            varn_core::RuntimeKind::Bool.name(),
            varn_core::RuntimeKind::Char.name(),
            varn_core::RuntimeKind::Map.name(),
            varn_core::RuntimeKind::Set.name(),
            varn_core::RuntimeKind::Range.name(),
            varn_core::well_known::ERROR,
            varn_core::well_known::TYPE_ERROR,
            varn_core::well_known::RANGE_ERROR,
            varn_core::RuntimeErrorKind::IntegerOverflow.class_name(),
            varn_core::RuntimeErrorKind::DivisionByZero.class_name(),
            varn_core::RuntimeErrorKind::MatchError.class_name(),
        ];
        for name in names {
            if let Some(nv) = self.globals_ref().get_by_name(name) {
                if let Some(obj) = self.heap.get(nv.as_heap_idx()) {
                    match obj {
                        crate::heap::HeapObj::Class(cls) => {
                            let cls = cls.clone();
                            self.heap.set_intrinsic_class(name, cls);
                        }
                        crate::heap::HeapObj::NativeFn(f, _) => {
                            let f = *f;
                            if let Ok(class_nv) = (f)(self as &mut dyn NativeCtx, &[]) {
                                if let Some(crate::heap::HeapObj::Class(cls)) =
                                    self.heap.get(class_nv.as_heap_idx())
                                {
                                    let cls = cls.clone();
                                    self.heap.set_intrinsic_class(name, cls);
                                    self.globals_mut().set_by_name(name, class_nv);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    #[allow(clippy::mut_from_ref)]
    pub(crate) fn globals_ref(&self) -> &GlobalStore {
        unsafe { &*self.globals.get() }
    }

    /// `&mut` out of the shared table under the same single-threaded
    /// contract as the heap: tasks on this thread take turns, never
    /// overlapping, so no aliasing `&mut` exists at once. Never hold the
    /// result across nested VM entry (`call_vm`/`invoke`).
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn globals_mut(&mut self) -> &mut GlobalStore {
        unsafe { &mut *self.globals.get() }
    }

    #[allow(clippy::mut_from_ref)]
    pub(crate) fn modules_ref(&self) -> &FxHashMap<ModuleId, VmValue> {
        unsafe { &*self.modules.get() }
    }

    pub(crate) fn fork_for_task(&self) -> Self {
        Self {
            stack: crate::frame_store::FrameStore::new_for_task(),
            frames: crate::frame_stack::FrameStack::with_capacity(8),
            globals: Rc::clone(&self.globals),
            heap: self.heap.clone(),
            try_handlers: Vec::new(),
            modules: Rc::clone(&self.modules),
            precompiled: Rc::clone(&self.precompiled),
            loader: self.loader.clone(),
            settings: self.settings,
            open_upvalues: Vec::new(),
            pending_constructors: Vec::new(),
            pending_setters: Vec::new(),
            vm_suspend: None,
            module_exports: FxHashMap::default(),
            opcode_counts: None,
            profile_counters: None,
            hotspot_counters: None,
            proto_constants: Rc::clone(&self.proto_constants),
            static_closures: Rc::clone(&self.static_closures),
            linker: self.linker.share(),
            jit_jmp_buf: std::ptr::null_mut(),
            jit_suspend_buf: std::ptr::null_mut(),
            jit_panic_exception_handler: None,
            jit_panic_exception_error: None,
            jit_panic_exception_err_obj: None,
            jit_panic_suspend_resume_ip: None,
            jit_native_result: VmValue::null(),
            osr_request: None,
            resources: Rc::clone(&self.resources),
            gc_inhibited: false,
            capabilities: Rc::clone(&self.capabilities),
            metadata: Rc::clone(&self.metadata),
            hashable_keys: Rc::clone(&self.hashable_keys),
            gc_root_scratch: Vec::new(),
            stage: Vec::new(),
            task_queue: std::cell::OnceCell::new(),
        }
    }
}

struct MinorSeg {
    owner: usize,
    kind: u8,
    start: usize,
    len: usize,
    aux: usize,
}

struct MinorRefSeg {
    owner: usize,
    start: usize,
    len: usize,
}

struct StashSeg {
    sowner: usize,
    frame: usize,
    is_refs: bool,
    src: usize,
    len: usize,
}

fn gather_minor_roots(
    ctx: &ExecCtx,
    owner: usize,
    vals: &mut Vec<VmValue>,
    refs: &mut Vec<u32>,
    segs: &mut Vec<MinorSeg>,
    refsegs: &mut Vec<MinorRefSeg>,
) {
    macro_rules! seg {
        ($kind:expr, $aux:expr, $body:block) => {{
            let start = vals.len();
            $body
            segs.push(MinorSeg {
                owner,
                kind: $kind,
                start,
                len: vals.len() - start,
                aux: $aux,
            });
        }};
    }
    seg!(0, 0, {
        // El almacén por clases dimensiona exacto por activación
        // (`push_frame` extiende, `pop_frame` trunca): los tramos vivos son
        // contiguos desde 0 y GPR/FPR ni se visitan — por construcción nunca
        // son raíces. Solo DYN se filtra por tag y REF va directo.
        vals.extend_from_slice(&ctx.stack.dyn_);
    });
    seg!(1, 0, {
        // Call-window staging: boxed args/receiver/result sitting here
        // between `exec_call_reg`'s slow path staging them and
        // `prepare_call`/`dispatch_prepared_call` consuming them are live
        // the same way any other pending value is — a heap ref parked here
        // when a nested allocation trips this same safepoint (e.g. building
        // a rest-args array) must survive nursery collection like everything
        // else, not just what already made it into a register.
        vals.extend_from_slice(&ctx.stage);
    });
    seg!(2, 0, {
        if owner == 0 {
            vals.extend_from_slice(&ctx.globals_ref().values);
        }
    });
    seg!(3, 0, {
        if owner == 0 {
            for v in ctx.modules_ref().values() {
                vals.push(*v);
            }
        }
    });
    seg!(4, 0, {
        for v in ctx.module_exports.values() {
            vals.push(*v);
        }
    });
    seg!(5, 0, {
        if owner == 0 {
            for (_, v) in unsafe { &*ctx.static_closures.get() }.values() {
                vals.push(*v);
            }
        }
    });
    if owner == 0 {
        for (key, (_, pool)) in unsafe { &*ctx.proto_constants.get() }.iter() {
            seg!(6, *key, {
                vals.extend_from_slice(pool);
            });
        }
    }
    seg!(7, 0, {
        for (_, v) in &ctx.pending_constructors {
            vals.push(*v);
        }
    });
    seg!(8, 0, {
        for (_, v) in &ctx.pending_setters {
            vals.push(*v);
        }
    });
    seg!(9, 0, {
        if let Some(VmSuspend::Yield { value, .. } | VmSuspend::Await { value, .. }) =
            &ctx.vm_suspend
        {
            vals.push(*value);
        }
    });
    seg!(10, 0, {
        if owner == 0 {
            for map in unsafe { &*ctx.metadata.get() }.values() {
                for &v in map.values() {
                    vals.push(v);
                }
            }
        }
    });
    seg!(11, 0, {
        if owner == 0 {
            for reps in unsafe { &*ctx.hashable_keys.get() }.values() {
                vals.extend_from_slice(reps);
            }
        }
    });
    seg!(12, 0, {
        vals.push(ctx.jit_native_result);
    });
    let ref_start = refs.len();
    refs.extend_from_slice(&ctx.stack.refs);
    refsegs.push(MinorRefSeg {
        owner,
        start: ref_start,
        len: ctx.stack.refs.len(),
    });
}

fn write_minor_seg(ctx: &mut ExecCtx, seg: &MinorSeg, vals: &[VmValue]) {
    let slice = &vals[seg.start..seg.start + seg.len];
    match seg.kind {
        0 => ctx.stack.dyn_.copy_from_slice(slice),
        1 => ctx.stage.copy_from_slice(slice),
        2 => ctx.globals_mut().values.copy_from_slice(slice),
        3 => {
            for (v, w) in unsafe { &mut *ctx.modules.get() }
                .values_mut()
                .zip(slice.iter())
            {
                *v = *w;
            }
        }
        4 => {
            for (v, w) in ctx.module_exports.values_mut().zip(slice.iter()) {
                *v = *w;
            }
        }
        5 => {
            for ((_, v), w) in unsafe { &mut *ctx.static_closures.get() }
                .values_mut()
                .zip(slice.iter())
            {
                *v = *w;
            }
        }
        6 => {
            if let Some((_, pool)) = unsafe { &mut *ctx.proto_constants.get() }.get_mut(&seg.aux) {
                Rc::make_mut(pool).copy_from_slice(slice);
            }
        }
        7 => {
            for ((_, v), w) in ctx.pending_constructors.iter_mut().zip(slice.iter()) {
                *v = *w;
            }
        }
        8 => {
            for ((_, v), w) in ctx.pending_setters.iter_mut().zip(slice.iter()) {
                *v = *w;
            }
        }
        9 => {
            if seg.len == 1 {
                if let Some(VmSuspend::Yield { value, .. } | VmSuspend::Await { value, .. }) =
                    &mut ctx.vm_suspend
                {
                    *value = slice[0];
                }
            }
        }
        10 => {
            let mut idx = 0;
            for map in unsafe { &mut *ctx.metadata.get() }.values_mut() {
                for v in map.values_mut() {
                    *v = slice[idx];
                    idx += 1;
                }
            }
        }
        11 => {
            let mut idx = 0;
            for reps in unsafe { &mut *ctx.hashable_keys.get() }.values_mut() {
                for v in reps.iter_mut() {
                    *v = slice[idx];
                    idx += 1;
                }
            }
        }
        _ => {
            ctx.jit_native_result = slice[0];
        }
    }
}

impl ExecCtx {
    pub fn run_minor_gc(&mut self) {
        // Union single-pass collection over this context plus every queued
        // fork. A per-fork collection would wipe the nursery before the
        // driver's own roots are updated, leaving them dangling (evacuate
        // fallback); gathering every root set first and collecting once
        // keeps one coherent forwarding table for all owners.
        let scope = super::scheduler::gc_scope(self);
        let mut owners = scope.owners;

        let mut all_vals = std::mem::take(&mut self.gc_root_scratch);
        all_vals.clear();
        let mut all_refs: Vec<u32> = Vec::new();
        let mut segs: Vec<MinorSeg> = Vec::new();
        let mut refsegs: Vec<MinorRefSeg> = Vec::new();
        let mut stash_segs: Vec<StashSeg> = Vec::new();
        for (owner_idx, owner_ptr) in owners.iter().enumerate() {
            // Shared borrow only; no mutation happens during gather, and no
            // collection runs until every owner contributed its roots.
            let ctx: &ExecCtx = unsafe { &**owner_ptr };
            gather_minor_roots(
                ctx,
                owner_idx,
                &mut all_vals,
                &mut all_refs,
                &mut segs,
                &mut refsegs,
            );
        }
        let mut sowners = scope.frozen;
        for (si, ptr) in sowners.iter().enumerate() {
            let st: &super::scheduler::Frozen = unsafe { &**ptr };
            if !st.has_refs {
                continue;
            }
            for (fi, sf) in st.frames().enumerate() {
                let start = all_vals.len();
                all_vals.extend_from_slice(&sf.dyn_);
                stash_segs.push(StashSeg {
                    sowner: si,
                    frame: fi,
                    is_refs: false,
                    src: start,
                    len: sf.dyn_.len(),
                });
                let rstart = all_refs.len();
                all_refs.extend_from_slice(&sf.refs);
                stash_segs.push(StashSeg {
                    sowner: si,
                    frame: fi,
                    is_refs: true,
                    src: rstart,
                    len: sf.refs.len(),
                });
            }
        }

        // TODO EL tramo, no solo `dyn_`: `all_vals` junta stack+stage+globals+
        // módulos+... precisamente para que cada uno cuente como raíz. Pasar
        // solo `[..dyn_len]` (como hacía esto) escaneaba los registros y
        // dejaba globals/static_closures/etc. sin tocar — el nursery los
        // wipea igual (`objects.clear()` es incondicional al final de
        // `collect`), así que cualquier objeto SOLO alcanzable desde un
        // global sobrevivía en el papel (la copia de vuelta no cambiaba nada)
        // pero desaparecía del heap: el primer `heap.get` posterior a un
        // minor GC con ese índice devolvía `None` — "invalid heap index" en
        // la siguiente llamada a una función de nivel de módulo.
        self.heap
            .minor_gc(&mut all_vals[..], &mut all_refs[..], &[]);

        for seg in segs.iter() {
            if seg.len == 0 {
                continue;
            }
            // Each owner is written back sequentially and exclusively; no
            // borrows overlap because only one `&mut` exists at a time.
            let ctx: &mut ExecCtx = unsafe { &mut *owners[seg.owner] };
            write_minor_seg(ctx, seg, &all_vals);
        }
        for refseg in refsegs.iter() {
            let ctx: &mut ExecCtx = unsafe { &mut *owners[refseg.owner] };
            let dst = &mut ctx.stack.refs[..refseg.len];
            dst.copy_from_slice(&all_refs[refseg.start..refseg.start + refseg.len]);
        }
        for sseg in stash_segs.iter() {
            let st: &mut super::scheduler::Frozen = unsafe { &mut *sowners[sseg.sowner] };
            if let Some(sf) = st.frame_mut(sseg.frame) {
                if sseg.is_refs {
                    if sf.refs.len() == sseg.len {
                        sf.refs
                            .copy_from_slice(&all_refs[sseg.src..sseg.src + sseg.len]);
                    }
                } else if sf.dyn_.len() == sseg.len {
                    sf.dyn_
                        .copy_from_slice(&all_vals[sseg.src..sseg.src + sseg.len]);
                }
            }
        }
        all_vals.clear();
        self.gc_root_scratch = all_vals;
    }

    /// Loop back-edge GC safepoint shared by the interpreter and the JIT.
    ///
    /// Suspended generator state is traced by the collectors themselves
    /// (`scan_and_fix_old_obj`'s Generator arm / the marker's Generator arm),
    /// so async liveness no longer defers collection. Only nested contexts
    /// (`gc_inhibited`) never initiate one — see that field's invariant.
    pub(crate) fn gc_backedge_safepoint(&mut self) {
        // Verifica el contrato v2 en debug: rangos contiguos, topes >= bases.
        // Costo cero en release.
        debug_assert!(crate::frame_store_abi::debug_check_stacks(&self.stack));
        if self.gc_inhibited {
            return;
        }
        if self.heap.needs_minor_gc() {
            self.run_minor_gc();
        }
        if self.heap.needs_gc() {
            self.trigger_gc();
        }
    }

    pub(crate) fn trigger_gc(&mut self) {
        if self.gc_inhibited {
            return;
        }
        self.run_minor_gc();
        let roots = self.major_roots();
        let _ = self.heap.collect(&roots);
    }

    /// Every heap index the context itself holds: the one root set a major
    /// collection starts from, wherever it is triggered.
    pub(crate) fn major_roots(&self) -> Vec<u32> {
        let mut roots: Vec<u32> = Vec::with_capacity(256);
        let scope = super::scheduler::gc_scope(self);
        for &owner in &scope.owners[1..] {
            roots.extend(unsafe { &*owner }.major_roots_local());
        }
        for cell in &scope.cells {
            cell.trace_cells(&mut |c| {
                let v = c.get();
                if v.is_heap() {
                    roots.push(v.as_heap_idx());
                }
            });
        }
        for lazy in &scope.lazies {
            lazy.trace_cells(&mut |c| {
                let v = c.get();
                if v.is_heap() {
                    roots.push(v.as_heap_idx());
                }
            });
        }
        for &frozen in &scope.frozen {
            let st: &super::scheduler::Frozen = unsafe { &*frozen };
            if !st.has_refs {
                continue;
            }
            for sf in st.frames() {
                for v in &sf.dyn_ {
                    if v.is_heap() {
                        roots.push(v.as_heap_idx());
                    }
                }
                for &h in &sf.refs {
                    if h != crate::frame_store::REF_UNINIT {
                        roots.push(h);
                    }
                }
            }
        }
        roots.extend(self.major_roots_local());
        for v in &self.globals_ref().values {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for v in self.modules_ref().values() {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for (_, v) in unsafe { &*self.static_closures.get() }.values() {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for (_, pool) in unsafe { &*self.proto_constants.get() }.values() {
            for v in pool.iter() {
                if v.is_heap() {
                    roots.push(v.as_heap_idx());
                }
            }
        }
        for map in unsafe { &*self.metadata.get() }.values() {
            for &v in map.values() {
                if v.is_heap() {
                    roots.push(v.as_heap_idx());
                }
            }
        }
        for reps in unsafe { &*self.hashable_keys.get() }.values() {
            for v in reps {
                if v.is_heap() {
                    roots.push(v.as_heap_idx());
                }
            }
        }
        roots
    }

    fn major_roots_local(&self) -> Vec<u32> {
        let mut roots: Vec<u32> = Vec::with_capacity(256);
        self.stack
            .collect_roots(self.stack.dyn_.len(), self.stack.refs.len(), &mut roots);
        for v in &self.stage {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        if self.jit_native_result.is_heap() {
            roots.push(self.jit_native_result.as_heap_idx());
        }
        for frame in &self.frames {
            for c in frame.closure().constants.iter() {
                if c.is_heap() {
                    roots.push(c.as_heap_idx());
                }
            }
        }
        for (_, v) in &self.pending_constructors {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for (_, v) in &self.pending_setters {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        roots
    }
}

pub use crate::arch::{vm_longjmp as my_longjmp, vm_setjmp as my_setjmp, JmpBuf};

#[cfg(test)]
mod gc_pool_tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn minor_gc_rewrites_cached_pools() {
        let mut ctx = ExecCtx::new(
            GlobalStore::default(),
            crate::settings::ExecSettings::default(),
        );
        let s = ctx.heap.alloc_str_interned("pool-gc-probe-fresh-string");
        let before = s.as_heap_idx();
        let proto = Rc::new(varn_types::FunctionProto::default());
        unsafe { &mut *ctx.proto_constants.get() }.insert(0x1234, (proto, Rc::new(vec![s])));
        ctx.run_minor_gc();
        let (_, pool) = &unsafe { &*ctx.proto_constants.get() }[&0x1234];
        let after = pool[0].as_heap_idx();
        assert!(ctx.heap.get(after).is_some());
        let _ = before;
    }

    #[test]
    fn major_roots_cover_cached_pools() {
        let mut ctx = ExecCtx::new(
            GlobalStore::default(),
            crate::settings::ExecSettings::default(),
        );
        let s = ctx.heap.alloc_str_interned("pool-gc-probe-major-string");
        let proto = Rc::new(varn_types::FunctionProto::default());
        unsafe { &mut *ctx.proto_constants.get() }.insert(0x1234, (proto, Rc::new(vec![s])));
        let roots = ctx.major_roots();
        assert!(roots.contains(&s.as_heap_idx()));
    }
}
