use super::*;

impl CellSpace {
    pub(super) fn sweep_block(&mut self, block: NonNull<Block>, may_return: bool) {
        let class = block_ref(block).class as usize;
        if self.check_allocs && self.classes[class].active == Some(block) {
            panic!("sweeping the active block");
        }
        let cell = CELL_CLASSES[class];
        let used = self.used_of(class, block);
        let mut live = 0usize;
        let mut freed = 0usize;
        let mut free_head = 0u64;
        for i in (0..used).rev() {
            let b = block_ref(block);
            let r = b.cell_at(i, cell);
            if b.is_marked(i) {
                live += 1;
            } else {
                let h = header(r);
                let state = SlotState::from_byte(h.state);
                if state != SlotState::Free {
                    if matches!(state, SlotState::Old | SlotState::Remembered) {
                        let bytes =
                            self.cell_bytes(r) + crate::heap::bytes::external_bytes(self, r);
                        self.old_bytes = self.old_bytes.saturating_sub(bytes);
                    }
                    if h.state & SIDE_TABLE != 0 {
                        self.record_dead(r);
                    }
                    if h.state & NEEDS_DROP != 0 {
                        unsafe { Self::drop_object(r) };
                    }
                    h.state = SlotState::Free as u8;
                    freed += 1;
                }
                unsafe { *blocks::next_free(r.addr()) = free_head };
                free_head = r.addr();
            }
        }
        self.swept_freed += freed as u64;
        let b = block_ref_mut(block);
        b.clear_marks();
        b.live = live as u32;
        b.free_head = free_head;
        b.swept = 1;
        if may_return && live == 0 {
            self.return_empty_block(class, block);
        }
    }

    fn return_empty_block(&mut self, class: usize, block: NonNull<Block>) {
        let cushion = self
            .nursery_bytes
            .max(self.block_bytes() / 10)
            .max(varn_types::cell::BLOCK_BYTES as u64);
        let mut empty_bytes = 0u64;
        for other in &self.classes {
            for &b in &other.blocks {
                if b != block && block_ref(b).live == 0 {
                    empty_bytes += varn_types::cell::BLOCK_BYTES as u64;
                }
            }
        }
        if empty_bytes + varn_types::cell::BLOCK_BYTES as u64 <= cushion {
            return;
        }
        Block::free(block);
        self.classes[class].blocks.retain(|&b| b != block);
    }

    pub(in crate::heap) fn clear_stale_marks(&mut self) {
        for class in &self.classes {
            for &block in &class.blocks {
                let b = block_ref_mut(block);
                if b.swept == 0 {
                    b.clear_marks();
                }
            }
        }
    }

    pub(in crate::heap) fn begin_major(&mut self) -> usize {
        let mut freed = 0;
        let large: Vec<HeapRef> = self.large.iter().map(|&(r, _)| r).collect();
        for r in large {
            let h = header(r);
            if h.state & MAJOR_MARK != 0 {
                h.state &= !MAJOR_MARK;
                if h.state & STATE_MASK != SlotState::Old as u8 {
                    set_age(r, SlotState::Old);
                }
            } else {
                self.release(r);
                freed += 1;
            }
        }
        let actives: Vec<usize> = self
            .classes
            .iter()
            .enumerate()
            .filter(|(_, class)| class.active.is_some())
            .map(|(index, _)| index)
            .collect();
        for class in actives {
            self.detach(class);
        }
        for class in &self.classes {
            for &block in &class.blocks {
                block_ref_mut(block).swept = 0;
            }
        }
        freed
    }

    pub(in crate::heap) fn sweep_some(&mut self, budget: usize) -> usize {
        if budget == 0 {
            return 0;
        }
        let mut done = 0;
        let mut dirty: Vec<NonNull<Block>> = Vec::new();
        for class in &self.classes {
            for &block in &class.blocks {
                if block_ref(block).swept == 0 {
                    dirty.push(block);
                }
            }
        }
        for block in dirty {
            if done >= budget {
                break;
            }
            if block_ref(block).swept == 0 {
                self.sweep_block(block, true);
                done += 1;
            }
        }
        done
    }

    pub(in crate::heap) fn force_sweep(&mut self) {
        let mut dirty: Vec<NonNull<Block>> = Vec::new();
        for class in &self.classes {
            for &block in &class.blocks {
                if block_ref(block).swept == 0 {
                    dirty.push(block);
                }
            }
        }
        for block in dirty {
            if block_ref(block).swept == 0 {
                self.sweep_block(block, false);
            }
        }
    }
}
