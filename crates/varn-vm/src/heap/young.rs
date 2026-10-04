//! The young generation: which slots were born since the last minor
//! collection, and which old slots were written a young reference since then.
//! A young object that survives stays in its slot and becomes old in place.

pub const YOUNG_THRESHOLD: usize = 49152;

#[derive(Default)]
pub struct YoungGen {
    pub(crate) born: Vec<u32>,
    pub(crate) remembered: Vec<u32>,
    pub(super) worklist: Vec<u32>,
    pub alloc_count: u64,
    pub minor_gc_count: u64,
    pub minor_gc_promoted: u64,
}

impl YoungGen {
    #[inline(always)]
    pub(crate) fn is_full(&self) -> bool {
        self.born.len() >= YOUNG_THRESHOLD
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.born.len()
    }

    /// Byte offset of the born-count (`born.len()`) inside `YoungGen`, for the
    /// JIT back-edge safepoint. Relies on Vec's (cap, ptr, len) word layout,
    /// validated against a live heap in `ExecCtx::new`.
    pub(crate) fn born_len_byte_offset() -> usize {
        std::mem::offset_of!(YoungGen, born) + 2 * std::mem::size_of::<usize>()
    }
}
