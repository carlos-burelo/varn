use super::VmValue;

/// Backing representation of a [`VmArray`]'s element buffer.
///
/// `#[repr(C, u8)]` pins a *defined* layout so the JIT can probe it: the
/// discriminant is a `u8` at offset 0, and each variant's `Vec` payload sits
/// at a fixed offset after the tag + alignment padding (8 on 64-bit targets).
/// The template and CLIF backends read the discriminant to guard their inline
/// fast paths and read the `Vec` words directly — see
/// `Heap::jit_array_layout` and the `discriminant() == Boxed` guards in the
/// JIT array paths.
///
/// A `VmArray` owns exactly one `Rc<UnsafeCell<ArrayRepr>>`, so every clone
/// and alias shares this single cell. An in-place migration (typed → `Boxed`
/// on a type-mismatched write) is therefore visible to *all* aliases and
/// never changes the array's identity — the `Rc` address (and thus the heap
/// index / `===` semantics / `Map` key) is preserved because only the cell's
/// *contents* are swapped, never the cell itself.
///
/// The variant is chosen from the *values*, not from a static type: a literal
/// picks its repr in [`VmArray::from_items`], and an array that starts empty
/// specializes on its first [`VmArray::push_vm`]. Nothing else can produce a
/// typed repr, and every typed repr is reversible — a mismatched write
/// migrates back to `Boxed` in place — so the representation is never
/// observable from the language.
#[repr(C, u8)]
#[derive(Debug)]
pub enum ArrayRepr {
    /// str / object / heterogeneous / `Dynamic` elements — tag+payload
    /// `VmValue` elements.
    Boxed(BoxedElems) = 0,
    /// `Array<int>` — raw `i64` buffer, holds no heap refs (GC skips it in A.2).
    I64(Vec<i64>) = 1,
    /// `Array<float>` — raw `f64` buffer, holds no heap refs.
    F64(Vec<f64>) = 2,
}

/// The element buffer of a `Boxed` array plus the length of its leading run
/// known to hold no nursery reference. The collector reads that run to skip
/// it: after a minor collection every element is old, and only a write below
/// the run, a removal that shortens it, or a raw `&mut Vec` hand-out lowers
/// it. `items` stays the first field so the JIT's raw reads of the `Vec`
/// words keep their offsets.
#[repr(C)]
#[derive(Debug)]
pub struct BoxedElems {
    pub(super) items: Vec<VmValue>,
    pub(super) clean_prefix: u32,
}

impl BoxedElems {
    #[inline(always)]
    pub fn new(items: Vec<VmValue>) -> Self {
        Self {
            items,
            clean_prefix: 0,
        }
    }

    #[inline(always)]
    pub fn as_vec(&self) -> &Vec<VmValue> {
        &self.items
    }
}

impl std::ops::Deref for BoxedElems {
    type Target = Vec<VmValue>;

    #[inline(always)]
    fn deref(&self) -> &Vec<VmValue> {
        &self.items
    }
}

impl ArrayRepr {
    /// Byte offset of the `repr(C, u8)` discriminant: first by construction.
    /// Tripwire-verified in `varn-vm` (`jit_array_layout` reads it back).
    pub const DISC_OFF: usize = 0;
    /// Byte offset where the element-`Vec` union opens: tag (1 B) + padding
    /// to `Vec` alignment (8). Inside it rigen las palabras medidas del `Vec`
    /// (`jit_array_layout` suma el word-offset probado una sola vez).
    pub const ELEMS_UNION_OFF: usize = 8;

    /// The `repr(C, u8)` discriminant (0 = Boxed, 1 = I64, 2 = F64). Matches the byte the JIT reads at offset 0
    /// of the `ArrayRepr`.
    #[inline(always)]
    pub fn discriminant(&self) -> u8 {
        match self {
            ArrayRepr::Boxed(_) => 0,
            ArrayRepr::I64(_) => 1,
            ArrayRepr::F64(_) => 2,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        match self {
            ArrayRepr::Boxed(v) => v.len(),
            ArrayRepr::I64(v) => v.len(),
            ArrayRepr::F64(v) => v.len(),
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

const _: () = {
    // La unión abre en el primer offset alineado a 8 tras el tag de 1 B.
    // Si la representación cambia, el tripwire de `jit_array_layout` lo
    // declara en voz alta en arranque, no el código emitido en caliente.
    assert!(std::mem::align_of::<ArrayRepr>() == 8);
    assert!(ArrayRepr::ELEMS_UNION_OFF == 8);
    assert!(std::mem::size_of::<ArrayRepr>().is_multiple_of(8));
};
