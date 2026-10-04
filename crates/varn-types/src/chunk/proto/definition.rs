use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::super::inline_cache::{FeedbackVector, PolyICSlot};
use super::super::literal::opt_rc_str_serde;
use super::super::Chunk;

pub type TrivialInitPlan = Rc<[(usize, u32, Option<varn_core::RuntimeKind>)]>;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct FunctionProto {
    #[serde(with = "opt_rc_str_serde")]
    pub name: Option<Arc<str>>,
    pub arity: usize,

    pub export_names: Vec<Arc<str>>,

    pub register_count: u16,
    pub has_rest: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub has_this: bool,
    pub upvalue_count: usize,
    pub cache_count: usize,
    pub chunk: Chunk,

    #[serde(default)]
    pub required_caps: Vec<std::sync::Arc<str>>,

    /// Palabras que ocupa el objeto de estado de esta función si es una
    /// máquina de estados; `0` si no lo es.
    ///
    /// Lo publica el proto —igual que `register_count`— para que el sitio de
    /// llamada pueda reservar el estado **sin conocer el callee en
    /// compilación**: lo lee en runtime. Eso es lo que mete al despacho
    /// dinámico (métodos de interfaz, callbacks, `dynamic`) en el camino de
    /// coste cero. Ver spec §3.8.
    ///
    /// **Dimensiona el objeto de estado; no decide si la función suspende.**
    /// `state_size == 1` no distingue dos casos reales: una `async` trivial
    /// que nunca suspende, y una `async` que sí suspende pero cuyo conjunto
    /// vivo a través de la suspensión está vacío (34 de los 127 puntos de
    /// suspensión del spec §3.8 son así — no es un caso raro). Un camino de
    /// coste cero construido leyendo sólo `state_size == 1` miscompilaría el
    /// segundo grupo entero. Si hace falta saber si la función suspende de
    /// verdad, la fuente es `varn_compiler::ssa::suspend::analyze`, no este
    /// campo.
    ///
    /// `#[serde(default)]` es para artefactos escritos por una build previa
    /// a la existencia de este campo — no protege la compatibilidad general
    /// del formato: `.vnc`/`.vnb`/`.vnm` serializan con `postcard`, que es
    /// posicional y no autodescriptivo, así que insertar un campo en medio
    /// del struct sin cambiar nada más desplazaría los campos siguientes en
    /// vez de aplicar el default. Lo que de verdad invalida las cachés
    /// viejas es `BUILD_FINGERPRINT` (`varn-modules/build.rs`), que cubre
    /// `varn-types`, `varn-modules` y `varn-checker` — los crates cuyos tipos
    /// entran en un payload — y por tanto cambia con cualquier cambio de forma
    /// de `FunctionProto`. (No cubre `varn-compiler`, que no aporta tipos
    /// serializados; de que el bytecode que ESE crate emite no sobreviva a su
    /// propio cambio responde `producer_fingerprint`, que sella las entradas
    /// de caché con la identidad del binario.)
    #[serde(default)]
    pub state_size: u16,

    /// Number of module-level global slots this proto's module owns, `0` for a
    /// non-module proto (nested function, closure). A module's globals occupy a
    /// contiguous region of the `GlobalStore` reserved when the module is
    /// evaluated; `LoadGlobalIdx`/`StoreGlobalIdx` carry the slot RELATIVE to
    /// that region's base, and the running closure carries the base. This is
    /// what let the runtime bytecode-rewrite pass (`globals::resolve`) go away:
    /// the checker already numbered these (`Resolution::GlobalSlot`), so the
    /// compiler emits the indexed form directly.
    ///
    /// Appended (postcard is positional) with `#[serde(default)]`; a cache
    /// written before this field decodes it as `0`, and `BUILD_FINGERPRINT`
    /// changes with the struct shape so those caches are discarded anyway.
    #[serde(default)]
    pub global_count: u32,

    #[serde(default)]
    pub register_meta: Vec<crate::register_meta::RegisterMeta>,

    #[serde(default)]
    pub exception_table: Vec<super::records::ExceptionRange>,

    /// Declared parameter slot kinds, in parameter order (from the checker
    /// via HirParam). The Cranelift router requires all-Int parameters
    /// before it may emit unboxed entry code.
    #[serde(default)]
    pub param_kinds: Vec<crate::register_meta::SlotKind>,

    /// Declared return slot kind. `Dynamic` when unannotated — the
    /// Cranelift wrapper may only re-tag when this proves Int.
    #[serde(default = "slot_kind_dynamic")]
    pub return_kind: crate::register_meta::SlotKind,

    /// Runtime cache: `PoolEntry::Shape` constants resolved to their
    /// `Shape` in the (globally cached) transition tree, so object literals
    /// don't re-derive the shape key-by-key on every allocation. Protos hold
    /// at most a handful of shape constants, so a linear scan beats hashing.
    #[serde(skip, default)]
    pub resolved_shapes: RefCell<Vec<(u32, Rc<crate::Shape>)>>,

    /// Address of this proto's compiled WRAPPER entry (the uniform `JitFn`
    /// ABI — see `varn_jit::JitFn`), or `0` if none is published yet. Was
    /// `Cell<Option<usize>>`; changed to match `clif_raw`'s convention below
    /// (0-sentinel, not `Option`) so JIT-generated code can load and compare
    /// this cell directly — `Option<usize>` has no spare bit pattern to niche
    /// its discriminant into (every `usize` bit pattern is a valid `usize`),
    /// so its in-memory shape is not something generated code could safely
    /// read without probing the compiler's choice out first, and a real
    /// entry address is never 0 either way.
    #[serde(skip)]
    #[serde(default)]
    pub jit_entry: std::cell::Cell<usize>,

    /// Address of this proto's Cranelift RAW entry — the unboxed
    /// `fn(exec_ctx, args…) -> i64` body, callable clif→clif without going
    /// back through the VM frame loop. `0` means "no direct entry": either
    /// the proto is not compiled yet, its compilation failed, or it took the
    /// frame-aware lowering (whose raw needs a callee frame the caller cannot
    /// supply).
    ///
    /// Call sites embed the ADDRESS OF THIS CELL and load it at run time
    /// rather than baking the entry in. Callers compile before their callees
    /// — a caller reaches its tier threshold first, by definition — so a
    /// compile-time snapshot would be `None` for essentially every call and
    /// would never be revisited. The extra load is what makes the direct call
    /// reachable at all.
    #[serde(skip)]
    #[serde(default)]
    pub clif_raw: std::cell::Cell<usize>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_code: std::cell::RefCell<Option<Rc<dyn std::any::Any>>>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_failed: std::cell::Cell<bool>,

    /// Which `ExecCtx` the code in `jit_entry`/`clif_raw` was compiled for.
    ///
    /// Compiled code is NOT context-independent: `LoadConst` bakes the
    /// constant's `VmValue` — a handle into one heap — as an immediate, and the
    /// linker bakes addresses of that context's globals and sibling protos.
    /// A proto, by contrast, outlives any single context: it is owned by the
    /// module chunk and survives every re-execution of the program. Running
    /// yesterday's code against today's heap reads whatever object now sits at
    /// the baked index — `"a" + <object> + "b"` where a literal belonged.
    /// A frame entry may only use the entry when this matches the running
    /// context's epoch; anything else recompiles.
    #[serde(skip)]
    #[serde(default)]
    pub jit_epoch: std::cell::Cell<u64>,

    /// When that code was built, on the VM's monotonic compile clock. A heap
    /// copied off another inherits the entries its ancestor had already built
    /// at the moment of the copy, and only those — this is what orders the two.
    #[serde(skip)]
    #[serde(default)]
    pub jit_serial: std::cell::Cell<u64>,

    /// Memoised "does this function contain a back edge": `0` not looked at,
    /// `1` yes, `2` no. Decides how the tier threshold applies — see
    /// [`Self::has_backedge`].
    #[serde(skip)]
    #[serde(default)]
    pub backedge_memo: std::cell::Cell<u8>,

    #[serde(skip)]
    #[serde(default)]
    pub resume_memo: std::cell::Cell<u8>,

    #[serde(skip, default = "proto_ic_default")]
    pub ic_cache: Rc<RefCell<Vec<PolyICSlot>>>,

    #[serde(skip, default = "proto_feedback_default")]
    pub feedback: Rc<RefCell<FeedbackVector>>,

    #[serde(skip)]
    #[serde(default)]
    pub frame_layout: std::cell::OnceCell<Rc<crate::register_meta::FrameLayout>>,

    #[serde(skip)]
    #[serde(default)]
    pub static_closure_val: std::cell::Cell<u64>,

    /// Frame entries seen so far, counted only while this proto is still
    /// uncompiled. Cranelift lowering costs ~640 µs per function against the
    /// ~17 µs the template JIT used to charge, so compiling at closure
    /// construction — as the template tier could afford — now dominates any
    /// workload that builds more functions than it runs (isolates: 144
    /// functions compiled to execute 38 JIT frames, 4.6 ms of interpretation
    /// turned into 60 ms). Compilation therefore waits for evidence the
    /// function is worth it.
    #[serde(skip)]
    #[serde(default)]
    pub jit_entry_count: std::cell::Cell<u32>,

    /// Back edges taken in this proto, across all frames. Drives the OSR
    /// trigger; unlike [`Self::jit_entry_count`] it keeps rising inside one
    /// long frame, which is the whole point — a function entered once and then
    /// looping reaches no entry threshold at all.
    #[serde(skip)]
    #[serde(default)]
    pub backedge_count: std::cell::Cell<u32>,

    /// Compiled ON-STACK REPLACEMENT entry: the same body lowered with a
    /// parameterless prologue that reloads every register from this frame's
    /// `ctx.stack` home slots and jumps straight to the block for
    /// [`Self::jit_osr_ip`].
    ///
    /// Valid ONLY for that ip, and only in the epoch recorded by
    /// [`Self::jit_osr_epoch`].
    #[serde(skip)]
    #[serde(default)]
    pub jit_osr_entry: std::cell::Cell<Option<usize>>,

    /// Which context [`Self::jit_osr_entry`] was baked for. Deliberately NOT
    /// [`Self::jit_epoch`]: that cell is re-stamped by
    /// `clif_link::adopt_if_inherited` when a copied heap adopts the NORMAL
    /// entry, and the adoption argument is per-entry — it tests
    /// [`Self::jit_serial`] against the copy's cutoff, which says nothing
    /// about an OSR variant compiled afterwards. Sharing the cell would let a
    /// copied heap enter code baked against its ancestor's objects. OSR never
    /// adopts; a mismatch just recompiles.
    #[serde(skip)]
    #[serde(default)]
    pub jit_osr_epoch: std::cell::Cell<u64>,

    /// The loop-header ip [`Self::jit_osr_entry`] resumes at. One OSR variant
    /// per proto: the first loop to prove hot wins, and a request for any other
    /// ip is refused rather than recompiling.
    #[serde(skip)]
    #[serde(default)]
    pub jit_osr_ip: std::cell::Cell<usize>,

    /// The OSR buffer, kept alive exactly like [`Self::jit_code`]: it is a
    /// separate `JitBuffer` from the normal entry's, and dropping it would
    /// unmap code the frame loop is about to jump into.
    #[serde(skip)]
    #[serde(default)]
    pub jit_osr_code: std::cell::RefCell<Option<Rc<dyn std::any::Any>>>,

    /// OSR was attempted and refused (ineligible shape, or Cranelift bailed).
    /// Latches so a frame that keeps looping does not re-attempt the same
    /// compilation every `JIT_OSR_BACKEDGES` back edges.
    #[serde(skip)]
    #[serde(default)]
    pub jit_osr_failed: std::cell::Cell<bool>,

    /// Cached plan for trivial field initialization constructors.
    #[serde(skip)]
    #[serde(default)]
    pub trivial_init_memo: std::cell::RefCell<Option<Option<TrivialInitPlan>>>,

    /// Portable typed SSA, attached after regalloc, or why the body has none
    /// (the JIT then lowers it from bytecode).
    #[serde(default)]
    pub ssa: crate::ssa::PortableSsa,

    /// Live physical registers at each suspension resume point, sorted by
    /// `resume_ip`. Only what the continuation can still read; the scheduler
    /// and GC root a parked frame through this instead of the whole frame.
    #[serde(default)]
    pub suspend_live: Vec<super::records::SuspendLive>,
}

fn slot_kind_dynamic() -> crate::register_meta::SlotKind {
    crate::register_meta::SlotKind::Dynamic
}

fn proto_ic_default() -> Rc<RefCell<Vec<PolyICSlot>>> {
    Rc::new(RefCell::new(Vec::new()))
}

fn proto_feedback_default() -> Rc<RefCell<FeedbackVector>> {
    Rc::new(RefCell::new(FeedbackVector::default()))
}
