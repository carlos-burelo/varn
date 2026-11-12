use super::*;

impl CellSpace {
    #[inline]
    pub(super) fn used_of(&self, class_index: usize, block: NonNull<Block>) -> usize {
        let b = block_ref(block);
        if self.classes[class_index].active == Some(block) {
            let cell = CELL_CLASSES[b.class as usize] as u64;
            ((self.classes[class_index].lane.bump - b.cells_start()) / cell) as usize
        } else {
            b.used as usize
        }
    }

    #[inline]
    pub(super) fn free_head_of(&self, class_index: usize, block: NonNull<Block>) -> u64 {
        if self.classes[class_index].active == Some(block) {
            self.classes[class_index].lane.free
        } else {
            block_ref(block).free_head
        }
    }

    pub(super) fn free_count_of(
        &self,
        class_index: usize,
        block: NonNull<Block>,
        cell: usize,
    ) -> usize {
        let mut n = 0;
        let mut r = self.free_head_of(class_index, block);
        while r != 0 && n <= cells_per_block(cell) {
            n += 1;
            r = unsafe { *blocks::next_free(r) };
        }
        n
    }

    fn new_block(&mut self, class: usize) -> NonNull<Block> {
        let block = Block::alloc(class as u8);
        self.classes[class].blocks.push(block);
        block
    }

    fn activate(&mut self, class: usize, block: NonNull<Block>) {
        let cell = CELL_CLASSES[class];
        let b = block_ref(block);
        let start = b.cells_start();
        let lane = &mut self.classes[class].lane;
        lane.free = b.free_head;
        lane.bump = start + b.used as u64 * cell as u64;
        lane.end = start + cells_span(cell);
        self.classes[class].active = Some(block);
    }

    pub(super) fn detach(&mut self, class: usize) {
        let Some(block) = self.classes[class].active else {
            return;
        };
        let cell = CELL_CLASSES[class];
        let lane = &mut self.classes[class].lane;
        let free = lane.free;
        let bump = lane.bump;
        lane.free = 0;
        lane.bump = 0;
        lane.end = 0;
        self.classes[class].active = None;
        let b = block_ref_mut(block);
        b.free_head = free;
        b.used = ((bump - b.cells_start()) / cell as u64) as u32;
    }

    #[inline]
    fn take(&mut self, class: usize) -> HeapRef {
        let cell = CELL_CLASSES[class];
        loop {
            if self.classes[class].active.is_some() {
                let lane = &mut self.classes[class].lane;
                if lane.free != 0 {
                    let r = lane.free;
                    lane.free = unsafe { *blocks::next_free(r) };
                    return unsafe { HeapRef::from_addr_unchecked(r) };
                }
                if lane.bump + cell as u64 <= lane.end {
                    let r = lane.bump;
                    lane.bump += cell as u64;
                    return unsafe { HeapRef::from_addr_unchecked(r) };
                }
                self.detach(class);
                continue;
            }
            let count = self.classes[class].blocks.len();
            let mut chosen = None;
            for i in 0..count {
                let block = self.classes[class].blocks[i];
                if block_ref(block).swept == 0 {
                    self.sweep_block(block, false);
                }
                let b = block_ref(block);
                if b.free_head != 0 || b.used < cells_per_block(cell) as u32 {
                    chosen = Some(block);
                    break;
                }
            }
            let block = match chosen {
                Some(block) => block,
                None => self.new_block(class),
            };
            self.activate(class, block);
        }
    }

    #[inline]
    pub(in crate::heap) fn take_cell(
        &mut self,
        body_bytes: usize,
        kind: u8,
        state: SlotState,
    ) -> HeapRef {
        let bytes = HEADER_BYTES + body_bytes;
        assert!(
            bytes <= u32::MAX as usize,
            "a single cell over 4 GiB is not supported"
        );
        let (r, class, aux) = match class_for(bytes) {
            Some(class) => (self.take(class), class as u8, 0u32),
            None => {
                let ptr = unsafe { alloc_zeroed(large_layout(bytes)) };
                let ptr = NonNull::new(ptr)
                    .unwrap_or_else(|| std::alloc::handle_alloc_error(large_layout(bytes)));
                let r = unsafe { HeapRef::from_addr_unchecked(ptr.as_ptr() as u64) };
                self.large.push((r, bytes));
                (r, CELL_CLASS_LARGE, bytes as u32)
            }
        };
        *header(r) = ObjHeader {
            state: state as u8,
            kind,
            class,
            _pad: 0,
            _aux: aux,
        };
        if state == SlotState::Old {
            self.old_births += 1;
        }
        if self.native {
            self.native_roots.push(r);
        }
        r
    }

    pub(crate) fn release(&mut self, r: HeapRef) {
        let h = header(r);
        let class = h.class;
        let raw_state = h.state;
        let state = SlotState::from_byte(raw_state);
        if self.check_allocs && state == SlotState::Free {
            panic!("double release of cell {r:?}");
        }
        if matches!(state, SlotState::Old | SlotState::Remembered) {
            let bytes = self.cell_bytes(r) + crate::heap::bytes::external_bytes(self, r);
            self.old_bytes = self.old_bytes.saturating_sub(bytes);
        }
        if raw_state & SIDE_TABLE != 0 && state != SlotState::Free {
            self.record_dead(r);
        }
        if h.state & NEEDS_DROP != 0 {
            unsafe { Self::drop_object(r) }
        }
        h.state = SlotState::Free as u8;
        if class == CELL_CLASS_LARGE {
            let at = self
                .large
                .iter()
                .position(|&(l, _)| l == r)
                .expect("a large cell");
            let (_, bytes) = self.large.swap_remove(at);
            unsafe { dealloc(r.as_ptr::<u8>(), large_layout(bytes)) };
        } else {
            let check = self.check_allocs;
            let block = block_of(r.addr());
            if check {
                let b = block_ref(block);
                let cell_size = CELL_CLASSES[b.class as usize];
                let start = b.cells_start();
                let end = start + cells_span(cell_size);
                if b.class != class {
                    panic!(
                        "released cell {r:?} with class {class} inside block of class {}",
                        b.class
                    );
                }
                if r.addr() < start || r.addr() >= end {
                    panic!(
                        "released cell {r:?} outside block cells (class {class}, state {raw_state}, kind {}, start {start:#x}, end {end:#x})",
                        h.kind
                    );
                }
            }
            let active = self.classes[class as usize].active == Some(block);
            let head = if active {
                self.classes[class as usize].lane.free
            } else {
                block_ref(block).free_head
            };
            unsafe { *blocks::next_free(r.addr()) = head };
            block_ref_mut(block).free_head = r.addr();
            if active {
                self.classes[class as usize].lane.free = r.addr();
            }
        }
    }
}
