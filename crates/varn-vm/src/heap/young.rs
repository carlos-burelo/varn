use varn_types::HeapRef;

pub struct YoungGen {
    pub(crate) threshold: usize,
    pub(crate) born: Vec<HeapRef>,
    pub(crate) remembered: Vec<HeapRef>,
    pub(super) worklist: Vec<HeapRef>,
    pub(crate) retired: u64,
    pub minor_gc_count: u64,
    pub minor_gc_promoted: u64,
}

impl YoungGen {
    pub(crate) fn new(threshold: usize) -> Self {
        Self {
            threshold,
            born: Vec::new(),
            remembered: Vec::new(),
            worklist: Vec::new(),
            retired: 0,
            minor_gc_count: 0,
            minor_gc_promoted: 0,
        }
    }
}

impl YoungGen {
    pub fn alloc_count(&self) -> u64 {
        self.retired + self.born.len() as u64
    }

    #[inline(always)]
    pub(crate) fn is_full(&self) -> bool {
        self.born.len() >= self.threshold
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.born.len()
    }

    pub(crate) fn born_len_byte_offset() -> usize {
        std::mem::offset_of!(YoungGen, born) + 2 * std::mem::size_of::<usize>()
    }
}
