use varn_types::HeapRef;

pub const YOUNG_THRESHOLD: usize = 49152;

#[derive(Default)]
pub struct YoungGen {
    pub(crate) born: Vec<HeapRef>,
    pub(crate) remembered: Vec<HeapRef>,
    pub(super) worklist: Vec<HeapRef>,
    pub(crate) retired: u64,
    pub minor_gc_count: u64,
    pub minor_gc_promoted: u64,
}

impl YoungGen {
    pub fn alloc_count(&self) -> u64 {
        self.retired + self.born.len() as u64
    }

    #[inline(always)]
    pub(crate) fn is_full(&self) -> bool {
        self.born.len() >= YOUNG_THRESHOLD
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.born.len()
    }

    pub(crate) fn born_len_byte_offset() -> usize {
        std::mem::offset_of!(YoungGen, born) + 2 * std::mem::size_of::<usize>()
    }
}
