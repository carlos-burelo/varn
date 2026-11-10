use super::*;

impl CellSpace {
    pub(crate) fn free_len(&self) -> usize {
        let mut total = 0;
        for (class_index, (class, cell)) in self.classes.iter().zip(CELL_CLASSES).enumerate() {
            for &block in &class.blocks {
                total += self.free_count_of(class_index, block, cell);
            }
        }
        total
    }

    pub(crate) fn capacity(&self) -> usize {
        let mut total = 0;
        for (class, cell) in self.classes.iter().zip(CELL_CLASSES) {
            total += class.blocks.len() * cells_per_block(cell);
        }
        total + self.large.len()
    }

    pub(crate) fn used_cells(&self) -> usize {
        let mut total = 0;
        for (class_index, class) in self.classes.iter().enumerate() {
            for &block in &class.blocks {
                total += self.used_of(class_index, block);
            }
        }
        total + self.large.len()
    }

    pub(crate) fn live_count(&self) -> usize {
        let mut total = 0;
        for (class_index, (class, cell)) in self.classes.iter().zip(CELL_CLASSES).enumerate() {
            for &block in &class.blocks {
                let b = block_ref(block);
                if b.swept == 0 {
                    total += self.used_of(class_index, block);
                } else {
                    total += self.used_of(class_index, block)
                        - self.free_count_of(class_index, block, cell);
                }
            }
        }
        total + self.large.len()
    }

    pub(crate) fn refs(&self) -> impl Iterator<Item = HeapRef> + '_ {
        (0..self.classes.len())
            .flat_map(move |class_index| {
                let class = &self.classes[class_index];
                class.blocks.iter().flat_map(move |&block| {
                    let b = block_ref(block);
                    let used = self.used_of(class_index, block);
                    let cell = CELL_CLASSES[b.class as usize];
                    (0..used).map(move |i| b.cell_at(i, cell))
                })
            })
            .filter(|&r| header(r).state & STATE_MASK != SlotState::Free as u8)
            .chain(self.large.iter().map(|&(r, _)| r))
    }

    pub(in crate::heap) fn block_bytes(&self) -> u64 {
        self.classes
            .iter()
            .map(|class| class.blocks.len())
            .sum::<usize>() as u64
            * varn_types::cell::BLOCK_BYTES as u64
            + self
                .large
                .iter()
                .map(|&(_, bytes)| bytes as u64)
                .sum::<u64>()
    }

    pub(crate) fn dirty_blocks(&self) -> usize {
        let mut n = 0;
        for class in &self.classes {
            n += class
                .blocks
                .iter()
                .filter(|&&block| block_ref(block).swept == 0)
                .count();
        }
        n
    }

    pub(crate) fn take_swept_identities(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.swept_identities)
    }

    pub(crate) fn take_swept_interned(&mut self) -> Vec<SweptInterned> {
        std::mem::take(&mut self.swept_interned)
    }

    pub(crate) fn free_list_problems(&self, problems: &mut Vec<String>) -> usize {
        let mut total = 0;
        for (class_index, (class, cell)) in self.classes.iter().zip(CELL_CLASSES).enumerate() {
            for &block in &class.blocks {
                let b = block_ref(block);
                let bound = self.used_of(class_index, block);
                let mut addr = self.free_head_of(class_index, block);
                let mut seen = 0;
                while addr != 0 {
                    if seen >= bound {
                        problems.push(format!(
                            "free list of block {:#x} (class {cell}) loops after {bound} cells",
                            b.base()
                        ));
                        break;
                    }
                    let start = b.cells_start();
                    let span = bound as u64 * cell as u64;
                    if addr < start
                        || addr >= start + span
                        || !(addr - start).is_multiple_of(cell as u64)
                    {
                        problems.push(format!(
                            "free list of block {:#x} (class {cell}) holds {addr:#x}, outside its cells",
                            b.base()
                        ));
                        break;
                    }
                    let h = header(unsafe { HeapRef::from_addr_unchecked(addr) });
                    if h.state & STATE_MASK != SlotState::Free as u8 {
                        problems.push(format!(
                            "free list of block {:#x} (class {cell}) holds live cell {addr:#x} (state {})",
                            b.base(),
                            h.state
                        ));
                        break;
                    }
                    addr = unsafe { *blocks::next_free(addr) };
                    seen += 1;
                }
                total += seen;
            }
        }
        total
    }
}

impl Drop for CellSpace {
    fn drop(&mut self) {
        if !self.abandon_objects {
            let live: Vec<HeapRef> = self.refs().collect();
            for r in live {
                if header(r).kind == varn_types::cell::CELL_KIND_INSTANCE {
                    continue;
                }
                unsafe { Self::drop_object(r) };
            }
        }
        for class in &self.classes {
            for &block in &class.blocks {
                Block::free(block);
            }
        }
        for &(r, bytes) in &self.large {
            unsafe { dealloc(r.as_ptr::<u8>(), large_layout(bytes)) };
        }
    }
}
