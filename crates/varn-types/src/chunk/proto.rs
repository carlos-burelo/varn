//! `FunctionProto`: one compiled function -- its chunk, register layout,
//! upvalues, exception ranges and the caches attached to it.

use std::cell::RefCell;
use std::rc::Rc;

use varn_core::OpCode;

use super::inline_cache::{FeedbackVector, PolyICSlot};
use super::literal::opt_rc_str_serde;
use super::pool::PoolEntry;
use super::Chunk;

/// Discriminante en `state[0]` del objeto de estado de una máquina de
/// estados (ver [`FunctionProto::state_size`]) cuando la máquina terminó: el
/// retorno de `poll` es el resultado final.
///
/// La convención `Poll` completa:
///
/// | `state[0]`        | significado                                     |
/// |--------------------|--------------------------------------------------|
/// | `STATE_DONE`        | `Ready`; el retorno es el resultado             |
/// | `STATE_YIELDED`      | `Yielded`; el retorno es lo emitido             |
/// | `>= FIRST_RESUME`   | `Pending`; el número es el punto de reanudación |
///
/// Vive aquí, en `varn-types`, y no en `varn-compiler` (que la definía
/// originalmente): es un contrato de RUNTIME — el sitio de llamada y el
/// scheduler leen `state[0]` tras invocar la función para decidir si
/// terminó, emitió o quedó suspendida — y ni `varn-vm` ni `varn-runtime`
/// dependen de `varn-compiler`, así que ninguno de los dos podría importarla
/// desde ahí sin duplicarla.
///
/// Cubre sólo uno de los dos productores de `Poll` que define el spec §3.8:
/// el **estado de corrutina**, que es lo que produce el pase
/// `state_machine`. El otro productor es el **futuro hoja del host**
/// (`crate::task::AsyncTask`), que no tiene `state[0]` — no es una función
/// del VM con objeto de estado propio — así que el scheduler necesita un
/// `poll` nativo aparte para ése.
pub const STATE_DONE: u32 = 0;
/// Ver [`STATE_DONE`].
pub const STATE_YIELDED: u32 = 1;
/// Primer discriminante de `state[0]` que denota un punto de reanudación
/// (`Pending`); el número es el punto de reanudación. Ver [`STATE_DONE`].
pub const FIRST_RESUME: u32 = 2;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ExceptionRange {
    pub try_start_ip: u32,
    pub try_end_ip: u32,
    pub catch_ip: u32,
    pub err_reg: u8,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FunctionProto {
    #[serde(with = "opt_rc_str_serde")]
    pub name: Option<Rc<str>>,
    pub arity: usize,

    pub export_names: Vec<Rc<str>>,

    pub register_count: u16,
    pub has_rest: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub has_this: bool,
    pub upvalue_count: usize,
    pub cache_count: usize,
    pub chunk: Chunk,

    #[serde(default)]
    pub required_caps: Vec<std::rc::Rc<str>>,

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

    #[serde(default)]
    pub register_meta: Vec<crate::register_meta::RegisterMeta>,

    #[serde(default)]
    pub exception_table: Vec<ExceptionRange>,

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

    #[serde(skip)]
    #[serde(default)]
    pub jit_entry: std::cell::Cell<Option<usize>>,

    /// Which `GlobalStore` this proto's global accesses are already bound to,
    /// or `0` for none. Set by `varn_vm::globals::resolve_in_proto`, which
    /// skips a proto whose id already matches the store it is handed.
    ///
    /// Not a "resolved yet?" bool: slot indices are only meaningful inside one
    /// store, and a VM (an isolate worker, say) that inherits a proto resolved
    /// against a different store must redo the work, not trust it. Recording
    /// the identity is what makes the fast path safe.
    ///
    /// Skipped by serde: it names a store that exists in one process only.
    #[serde(skip)]
    #[serde(default)]
    pub globals_id: std::cell::Cell<u64>,

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

    #[serde(skip, default = "proto_ic_default")]
    pub ic_cache: Rc<RefCell<Vec<PolyICSlot>>>,

    #[serde(skip, default = "proto_feedback_default")]
    pub feedback: Rc<RefCell<FeedbackVector>>,

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

impl FunctionProto {
    /// Whether the body contains a back edge (`OpCode::Loop`).
    ///
    /// Tiering counts FRAME ENTRIES, which says nothing about a function that
    /// is entered once and then spins a million iterations: it never reaches
    /// any threshold, and without on-stack replacement there is no second
    /// chance to compile it. So a looping function is compiled on its first
    /// entry and only straight-line code is made to prove itself by being
    /// called again — for that shape the entry count is exactly the right
    /// evidence. Measured: a flat threshold of 8 is 5.6x on the test suite and
    /// 2.4x WORSE on `bench_matrix`; splitting the two recovers both.
    ///
    /// Walked through the shared decoder so operand words can never be
    /// mistaken for an opcode, and memoised — the answer is a property of the
    /// bytecode, which never changes.
    pub fn has_backedge(&self) -> bool {
        match self.backedge_memo.get() {
            1 => return true,
            2 => return false,
            _ => {}
        }
        let code = &self.chunk.code;
        let mut ip = 0;
        let mut found = false;
        while ip < code.len() {
            let Some(info) = crate::bytecode::decode(code, ip, &self.chunk.constants) else {
                // Undecodable: assume the worst and compile eagerly, which is
                // the pre-existing behaviour.
                found = true;
                break;
            };
            if OpCode::from_u16(code[ip]) == Some(OpCode::Loop) {
                found = true;
                break;
            }
            ip += info.len.max(1);
        }
        self.backedge_memo.set(if found { 1 } else { 2 });
        found
    }

    pub fn ensure_ic(&self) {
        let n = self.cache_count;
        if n == 0 {
            return;
        }
        let mut ic = self.ic_cache.borrow_mut();
        if ic.is_empty() {
            ic.resize_with(n, PolyICSlot::new);
        }
        let mut fb = self.feedback.borrow_mut();
        if fb.sites.is_empty() {
            *fb = FeedbackVector::new(n);
        }
    }

    /// Resolve the `PoolEntry::Shape` at `idx` to its runtime `Shape`. The
    /// first call derives it through the transition tree (which caches each
    /// step globally); later calls hit the per-proto cache.
    pub fn resolved_shape(&self, idx: usize) -> Option<Rc<crate::Shape>> {
        if let Some((_, s)) = self
            .resolved_shapes
            .borrow()
            .iter()
            .find(|(i, _)| *i as usize == idx)
        {
            return Some(Rc::clone(s));
        }
        let keys = match self.chunk.constants.get(idx) {
            Some(PoolEntry::Shape(k)) => k,
            _ => return None,
        };
        let mut shape = crate::root_shape();
        for k in keys {
            shape = shape.transition(Rc::clone(k));
        }
        self.resolved_shapes
            .borrow_mut()
            .push((idx as u32, Rc::clone(&shape)));
        Some(shape)
    }
}

impl PartialEq for FunctionProto {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.arity == other.arity
            && self.export_names == other.export_names
            && self.register_count == other.register_count
            && self.has_rest == other.has_rest
            && self.is_async == other.is_async
            && self.is_generator == other.is_generator
            && self.has_this == other.has_this
            && self.upvalue_count == other.upvalue_count
            && self.cache_count == other.cache_count
            && self.chunk == other.chunk
            && self.required_caps == other.required_caps
            && self.state_size == other.state_size
    }
}

impl Eq for FunctionProto {}

impl std::hash::Hash for FunctionProto {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.arity.hash(state);
        self.export_names.hash(state);
        self.register_count.hash(state);
        self.has_rest.hash(state);
        self.is_async.hash(state);
        self.is_generator.hash(state);
        self.has_this.hash(state);
        self.upvalue_count.hash(state);
        self.cache_count.hash(state);
        self.chunk.hash(state);
        self.required_caps.hash(state);
        self.state_size.hash(state);
    }
}
