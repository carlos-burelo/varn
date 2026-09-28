//! JIT ABI v2 — único contrato caliente (`docs/JIT_ABI_V2_SPEC.md` §2).
//!
//! Crate hoja: solo depende de `varn-core`. `varn-jit` y `varn-vm` lo importan;
//! ninguno lo redefine ni re-deriva (Ley 3, Ley 6). El JIT direcciona **solo**
//! dentro de [`AbiCtx`]; lo demás es memoria privada de la VM.
//!
//! Invariantes que este archivo protege:
//! - Stacks de registros con bases estables (reserva virtual, nunca `Vec` que
//!   realoje): el JIT lee la base una vez por función, cero recargas.
//! - Frames de tamaño fijo en arena bump; **el caller empuja siempre** (sin
//!   handshake `frame_prepushed`).
//! - `Rc`/`Option`/`Cell` no cruzan: solo punteros crudos, índices `u32` y
//!   estos structs. `VmValue` (16 B tag+payload) es lo único viejo que
//!   sobrevive; aquí se espeja como [`AbiValue`] sin duplicar su lógica.

use core::mem::{align_of, offset_of, size_of};

use varn_core as _;

// ── Magia y versión ──────────────────────────────────────────────────────

/// Firma del contrato. Un código horneado contra otro `epoch`/versión nunca
/// ejecuta: la caché invalida el lote entero (un compare por entrada, §3.2).
pub const ABI_MAGIC: u32 = 0x5641_524E; // "VARN"
/// Versión del contrato. Sin retrocompatibilidad: si cambia, se borra y se
/// reescribe (Ley 8).
pub const ABI_VERSION: u16 = 2;

/// Índices por `SlotClass::index()` (`varn-types`). Duplicados aquí como
/// constantes porque esta crate no depende de `varn-types` (hoja); el lado VM
/// los verifica con `debug_assert` al construir [`ActBases`].
pub const CLASS_GPR: usize = 0;
pub const CLASS_FPR: usize = 1;
pub const CLASS_REF: usize = 2;
pub const CLASS_DYN: usize = 3;

// ── Valor ────────────────────────────────────────────────────────────────

/// Espejo `#[repr(C)]` de `VmValue` (tag + payload, 16 B). Sin lógica de
/// kinds: esa vive en `varn-types` (única fuente, Ley 6). Conversiones por
/// partes crudas, costo cero.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbiValue {
    pub tag: u64,
    pub payload: u64,
}

impl AbiValue {
    #[inline(always)]
    pub const fn from_raw_parts(tag: u64, payload: u64) -> Self {
        Self { tag, payload }
    }
    #[inline(always)]
    pub const fn null() -> Self {
        Self { tag: 0, payload: 0 }
    }
}

// ── Cabecera ─────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiHeader {
    pub magic: u32,
    pub version: u16,
    pub size: u16,
}

impl AbiHeader {
    pub const fn current() -> Self {
        Self {
            magic: ABI_MAGIC,
            version: ABI_VERSION,
            size: size_of::<AbiCtx>() as u16,
        }
    }
    #[inline(always)]
    pub const fn is_current(&self) -> bool {
        self.magic == ABI_MAGIC && self.version == ABI_VERSION
    }
}

// ── Stacks: 4 bases estables + topes ─────────────────────────────────────

/// Ventanas bump por clase. `base` no se mueve jamás en el proceso (reserva
/// virtual, se confirma por páginas); el JIT la conserva en registros.
/// Desaparece "recargar el puntero tras llamada que pudo realojar".
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiStacks {
    pub gpr: *mut i64,
    pub gpr_end: *mut i64,
    pub fpr: *mut f64,
    pub fpr_end: *mut f64,
    pub refs: *mut u32,
    pub refs_end: *mut u32,
    pub dyn_: *mut AbiValue,
    pub dyn_end: *mut AbiValue,
}

// ── Frames: arena bump, caller empuja ────────────────────────────────────

/// Bases por clase de una activación, indexadas por clase (§2).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ActBases {
    pub bases: [u32; 4],
}

/// Vista C de un closure. Nunca `VmClosure`: sin `Rc`, sin mapas, sin
/// contadores. Lo que la VM necesita del lado Rust vive en paralelo.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiClosure {
    pub proto: *const u8,
    pub module_base: u32,
    pub ic_stride: u32,
    pub ic_entries: *const u8,
}

/// Registro de activación de tamaño fijo. `resume`/`dest` SOLO los lee el
/// unwinder (`throw`, frío); el camino rápido no los escribe (§3.3: la
/// side-table por función los sustituye en caliente).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiFrame {
    pub closure: *const AbiClosure,
    pub caller: u32,
    pub resume: u32,
    pub dest: u16,
    pub _pad: u16,
    pub bases: ActBases,
}

/// Descriptor bump de activaciones: un check de capacidad por llamada.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiFrameArena {
    pub base: *mut AbiFrame,
    pub len: u32,
    pub cap: u32,
}

// ── Heap mínimo ──────────────────────────────────────────────────────────

/// Descriptor mínimo: umbral nursery + flag GC. Sin longitudes inline: el
/// colector arma `poll`; el código hace `test al,al; jnz slow` (§3.5).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiHeap {
    pub nursery_threshold: u64,
    pub gc_requested: u8,
    pub _pad: [u8; 7],
}

// ── Contexto ─────────────────────────────────────────────────────────────

/// Todo lo que el código generado toca. Firma única:
/// `raw(ctx: *mut AbiCtx, frame: *mut AbiFrame)`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiCtx {
    pub header: AbiHeader,
    pub epoch: u64,
    pub stacks: AbiStacks,
    pub frames: AbiFrameArena,
    pub heap: AbiHeap,
    pub result: AbiValue,
    pub poll: u8,
    pub _pad: [u8; 7],
}

impl AbiCtx {
    /// Safepoint de un byte (§3.5): `0 = seguir, !=0 = slow path`.
    #[inline(always)]
    pub fn should_poll(&self) -> bool {
        self.poll != 0
    }
}

/// Firma única JIT→JIT (§3.1). Sin args en registros: el caller materializó
/// los registros del callee en su tramo de stacks. Retorno escalar unboxed en
/// `rax`/`xmm0`, resto `void` + boxed en `ctx.result`.
pub type RawJitFn = unsafe extern "C" fn(ctx: *mut AbiCtx, frame: *mut AbiFrame);

/// Camino dinámico único (§3.2): métodos, closures, `dynamic`. Argumentos en
/// ventana contigua ya preparada por el caller. Sin IC inlineado a mano.
pub type InvokeDynamicFn =
    unsafe extern "C" fn(ctx: *mut AbiCtx, callee: AbiValue, argc: u32) -> AbiValue;

// ── Side-table de unwind (§3.3) ──────────────────────────────────────────

/// `{offset_pc_llamada → (resume_ip, dest)}` emitida en compilación.
/// Consultada SOLO por el unwinder en `throw` (frío). Cero stores por llamada.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CallSite {
    pub pc_offset: u32,
    pub resume_ip: u32,
    pub dest: u16,
    pub _pad: u16,
}

impl CallSite {
    /// Búsqueda lineal: tabla pequeña, camino frío. Sin hash (Ley 4).
    pub fn lookup(table: &[CallSite], pc_offset: u32) -> Option<(u32, u16)> {
        table
            .iter()
            .find(|e| e.pc_offset == pc_offset)
            .map(|e| (e.resume_ip, e.dest))
    }
}

// ── Asserts de compilación (§2, no negociables) ──────────────────────────

const _: () = {
    assert!(size_of::<AbiValue>() == 16, "AbiValue: 16 B tag+payload");
    assert!(align_of::<AbiValue>() == 8, "AbiValue: align 8");
    assert!(size_of::<AbiHeader>() == 8, "AbiHeader");
    assert!(offset_of!(AbiCtx, header) == 0, "AbiCtx.header");
    assert!(offset_of!(AbiCtx, epoch) == 8, "AbiCtx.epoch");
    assert!(offset_of!(AbiCtx, stacks) == 16, "AbiCtx.stacks");
    assert!(offset_of!(AbiCtx, result) % 8 == 0, "AbiCtx.result align");
    assert!(
        offset_of!(AbiCtx, poll) == offset_of!(AbiCtx, result) + 16,
        "AbiCtx.poll"
    );
    assert!(size_of::<AbiStacks>() == 64, "AbiStacks: 8 ptrs");
    assert!(size_of::<ActBases>() == 16, "ActBases: 4xu32");
    assert!(offset_of!(AbiFrame, closure) == 0, "AbiFrame.closure");
    assert!(offset_of!(AbiFrame, bases) == 20, "AbiFrame.bases");
    assert!(size_of::<AbiFrame>() == 40, "AbiFrame: tamaño fijo");
    assert!(size_of::<CallSite>() == 12, "CallSite");
    assert!(ABI_MAGIC == 0x5641_524E, "magic");
    assert!(ABI_VERSION == 2, "version");
};
