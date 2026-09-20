//! The per-register physical kind the backend lowers against.
//!
//! Etapa 5: this is the ONLY type representation the JIT reads. It is a
//! projection of the TIR's `BackendTy` onto what codegen actually
//! discriminates — an unboxed scalar (`Int` / `Float` / `Bool`), a value that
//! may be a small-string inline payload (`Str`), a value that is always a heap
//! reference (`Ref`), or a value with no static shape (`Dynamic`). The class /
//! array / nullable type handles the checker carries never reached codegen, so
//! they are not kept here.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SlotKind {
    Int,
    Float,
    Bool,
    /// A `str` — either a heap string or an inline small-string payload, so
    /// NOT unconditionally a heap pointer.
    Str,
    /// Always a heap reference: a class instance, an array, a map/set, an
    /// enum value, a closure, a decimal/bigint/tuple. Never null, never a
    /// scalar, never inline.
    Ref,
    Dynamic,
}

impl Default for SlotKind {
    /// `Dynamic` — matches `proto.rs`'s pre-existing `slot_kind_dynamic()`
    /// serde default for `FunctionProto::return_kind`, the only place this
    /// type's default previously had a name.
    fn default() -> Self {
        SlotKind::Dynamic
    }
}

/// The physical storage class a register is lowered against.
///
/// This is the single projection from the checker's [`SlotKind`] to the
/// register file, shared by both execution tiers so they can never disagree
/// about where a register lives: the interpreter's partitioned frame
/// (`varn-vm::frame_store`) and the JIT lowering both read it. Keeping it here
/// (rather than in the VM) is what lets the backend address a register by
/// `(class, index)` without re-deriving the mapping — Ley 3 (una tabla, un
/// dueño) for the register file.
///
/// `Int`/`Float`/`Ref` are their own classes; `Bool` and `Str` stay `Dyn`
/// deliberately (the next step, not this one): neither has an unpacked path
/// today — comparisons, `JumpIfFalse` and SSO/heap-str all read them boxed, so
/// moving them would change truthiness/representation semantics rather than
/// just relocate storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SlotClass {
    Gpr,
    Fpr,
    Ref,
    Dyn,
}

impl SlotClass {
    /// Honest projection of the checker's proof onto physical storage.
    #[inline(always)]
    pub fn of_kind(kind: SlotKind) -> Self {
        match kind {
            SlotKind::Int => SlotClass::Gpr,
            SlotKind::Float => SlotClass::Fpr,
            SlotKind::Ref => SlotClass::Ref,
            SlotKind::Dynamic | SlotKind::Bool | SlotKind::Str => SlotClass::Dyn,
        }
    }

    #[inline(always)]
    pub fn index(self) -> usize {
        match self {
            SlotClass::Gpr => 0,
            SlotClass::Fpr => 1,
            SlotClass::Ref => 2,
            SlotClass::Dyn => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct RegisterMeta {
    pub kind: SlotKind,
}


/// Sentinel stored in a `Ref` slot that has never been written.
///
/// The interpreter over-dimensions `register_count`, so trailing `Ref` slots
/// are allocated but never written; the GC skips this value and no reader
/// reaches them before a write (the same discipline the old `null()` slot
/// relied on). Lives here, next to the class projection, because the JIT's
/// emitted ref stores/loads must use the exact same sentinel as the VM's
/// `FrameStore` — a divergence would turn "unwritten" into a bogus heap index.
pub const REF_UNINIT: u32 = u32::MAX;

/// Static register → `(class, index-within-class)` mapping for one proto.
///
/// A pure projection of [`FunctionProto::register_meta`] (via [`SlotClass`]),
/// so both execution tiers compute the same home-slot coordinates: the VM's
/// partitioned frame uses it to place each activation's registers, and the
/// JIT lowering uses it to emit `class_vec[base[class] + idx]`. Deterministic
/// by construction — a linear scan in register order, no hashing (Ley 4).
#[derive(Debug)]
pub struct FrameLayout {
    /// `slots[reg] = (class, index within that class)`.
    pub slots: Vec<(SlotClass, u32)>,
    /// How many slots each class owns (`counts[class.index()]`).
    pub counts: [u32; 4],
}

impl FrameLayout {
    pub fn for_proto(proto: &crate::FunctionProto) -> Self {
        let n = proto.register_count as usize;
        let mut slots = Vec::with_capacity(n);
        let mut counts = [0u32; 4];
        for r in 0..n {
            let kind = proto
                .register_meta
                .get(r)
                .map(|m| m.kind)
                .unwrap_or(SlotKind::Dynamic);
            let class = SlotClass::of_kind(kind);
            let idx = counts[class.index()];
            counts[class.index()] += 1;
            slots.push((class, idx));
        }
        Self { slots, counts }
    }

    #[inline(always)]
    pub fn class_of(&self, reg: usize) -> SlotClass {
        self.slots
            .get(reg)
            .map(|(c, _)| *c)
            .unwrap_or(SlotClass::Dyn)
    }

    #[inline(always)]
    pub fn idx_of(&self, reg: usize) -> u32 {
        self.slots.get(reg).map(|(_, i)| *i).unwrap_or(0)
    }
}
