use varn_types::HeapRef;

pub struct YoungGen {
    pub(crate) limit_cells: u64,
    pub(crate) bytes: u64,
    pub(crate) born: Vec<HeapRef>,
    pub(crate) remembered: Vec<HeapRef>,
    pub(super) worklist: Vec<HeapRef>,
    pub(crate) retired: u64,
    pub minor_gc_count: u64,
    pub minor_gc_promoted: u64,
}

impl YoungGen {
    pub(crate) fn new() -> Self {
        Self {
            limit_cells: 0,
            bytes: 0,
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
        self.born.len() as u64 >= self.limit_cells
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.born.len()
    }

    pub(crate) fn born_len_byte_offset() -> usize {
        std::mem::offset_of!(YoungGen, born) + varn_types::cell::VEC_LEN_OFF
    }

    pub(crate) fn limit_byte_offset() -> usize {
        std::mem::offset_of!(YoungGen, limit_cells)
    }

    pub(crate) fn probe_born_layout() {
        use varn_types::cell::{VEC_CAP_OFF, VEC_LEN_OFF, VEC_PTR_OFF};
        const LEN: usize = 3;
        const CAP: usize = 7;
        let mut probe: Vec<HeapRef> = Vec::with_capacity(CAP);
        for i in 0..LEN {
            probe.push(unsafe { HeapRef::from_addr_unchecked(0x1000 + i as u64 * 8) });
        }
        assert_eq!(
            std::mem::size_of::<Vec<HeapRef>>(),
            3 * std::mem::size_of::<usize>(),
            "born layout probe: Vec<HeapRef> is not three words"
        );
        let words: [usize; 3] = unsafe { std::mem::transmute_copy(&probe) };
        let stride = std::mem::size_of::<usize>();
        let (mut saw_ptr, mut saw_len, mut saw_cap) = (false, false, false);
        for (i, &word) in words.iter().enumerate() {
            let off = i * stride;
            if word == probe.as_ptr() as usize {
                assert_eq!(
                    off, VEC_PTR_OFF,
                    "born layout probe: Vec<HeapRef> pointer is not at VEC_PTR_OFF, which the JIT reads"
                );
                saw_ptr = true;
            } else if word == LEN {
                assert_eq!(
                    off, VEC_LEN_OFF,
                    "born layout probe: Vec<HeapRef> length is not at VEC_LEN_OFF, which the JIT reads"
                );
                saw_len = true;
            } else if word == CAP {
                assert_eq!(
                    off, VEC_CAP_OFF,
                    "born layout probe: Vec<HeapRef> capacity is not at VEC_CAP_OFF, which the JIT reads"
                );
                saw_cap = true;
            }
        }
        assert!(
            saw_ptr && saw_len && saw_cap,
            "born layout probe: Vec<HeapRef> words were not identified"
        );
        assert_eq!(
            Self::born_len_byte_offset(),
            std::mem::offset_of!(Self, born) + VEC_LEN_OFF,
            "born layout probe: born length offset disagrees with the Vec layout"
        );
    }
}
