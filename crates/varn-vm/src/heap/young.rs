use varn_types::HeapRef;

const DEFAULT_YOUNG_THRESHOLD: usize = 786432;
const DEFAULT_OLD_FLOOR: u64 = 65536;

fn stress_cells() -> Option<usize> {
    static STRESS: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    *STRESS.get_or_init(|| varn_core::env::settings().gc_stress)
}

pub fn young_threshold() -> usize {
    stress_cells().unwrap_or(DEFAULT_YOUNG_THRESHOLD)
}

pub fn old_floor() -> u64 {
    stress_cells().map_or(DEFAULT_OLD_FLOOR, |cells| cells as u64)
}

pub struct YoungGen {
    pub(crate) threshold: usize,
    pub(crate) born: Vec<HeapRef>,
    pub(crate) remembered: Vec<HeapRef>,
    pub(super) worklist: Vec<HeapRef>,
    pub(crate) retired: u64,
    pub minor_gc_count: u64,
    pub minor_gc_promoted: u64,
}

impl Default for YoungGen {
    fn default() -> Self {
        Self {
            threshold: young_threshold(),
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
