use std::cell::RefCell;

/// Region-based Request Arena for ultra-low latency, zero-GC per-request allocations.
///
/// Objects allocated inside a request scope are bump-allocated contiguously in memory.
/// When the request completes, resetting the arena (`reset()`) reclaims all memory
/// in a single O(1) pointer reset (`offset = 0`), yielding 0.00 ms GC pauses during HTTP requests.
pub struct RequestArena {
    buf: Vec<u8>,
    offset: usize,
}

impl RequestArena {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: vec![0u8; cap],
            offset: 0,
        }
    }

    #[inline]
    pub fn alloc_bytes(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        let align_mask = align - 1;
        let aligned_offset = (self.offset + align_mask) & !align_mask;
        if aligned_offset + size <= self.buf.len() {
            let ptr = unsafe { self.buf.as_mut_ptr().add(aligned_offset) };
            self.offset = aligned_offset + size;
            Some(ptr)
        } else {
            None
        }
    }

    #[inline]
    pub fn reset(&mut self) {
        self.offset = 0;
    }

    #[inline]
    pub fn allocated_bytes(&self) -> usize {
        self.offset
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }
}

thread_local! {
    static CURRENT_ARENA: RefCell<Option<RequestArena>> = const { RefCell::new(None) };
}

pub fn enter_request_arena(capacity: usize) {
    CURRENT_ARENA.with(|a| {
        *a.borrow_mut() = Some(RequestArena::with_capacity(capacity));
    });
}

pub fn reset_request_arena() {
    CURRENT_ARENA.with(|a| {
        if let Some(ref mut arena) = *a.borrow_mut() {
            arena.reset();
        }
    });
}

pub fn exit_request_arena() {
    CURRENT_ARENA.with(|a| {
        *a.borrow_mut() = None;
    });
}

pub fn request_arena_stats() -> Option<(usize, usize)> {
    CURRENT_ARENA.with(|a| {
        a.borrow()
            .as_ref()
            .map(|arena| (arena.allocated_bytes(), arena.capacity()))
    })
}
