//! Builds [`crate::gc_report::GcReport`] by actually reading `HeapInner` —
//! split from `crate::gc_report` (the plain-data shape) because this is the
//! one file that needs `pub(super)` access to the heap's private tables.

use super::cells::SlotState;
use super::obj::HeapObj;
use super::structs::Heap;
use crate::gc_report::{GcReport, HistogramRow, InternerSizes, OldGenReport, YoungReport};
use rustc_hash::FxHashMap;

impl Heap {
    pub fn gc_report(&self) -> GcReport {
        let inner = unsafe { &*self.inner.get() };
        let young = &inner.young;

        let young_report = YoungReport {
            threshold: super::young::YOUNG_THRESHOLD,
            live: young.len(),
            alloc_count: young.alloc_count,
            minor_gc_count: young.minor_gc_count,
            minor_gc_promoted: young.minor_gc_promoted,
        };

        let old_gen_report = OldGenReport {
            slots_total: inner.cells.capacity(),
            slots_live: inner.cells.live_count(),
            free_list: inner.cells.free_len(),
            alloc_count: inner.cells.births,
            gc_collections: inner.gc_collections,
            gc_total_freed: inner.gc_total_freed,
            gc_alloc_since_collect: inner.cells.old_growth,
            gc_threshold: inner.gc_threshold,
        };

        let interners = InternerSizes {
            strings: inner.string_interner.len(),
            symbols: inner.symbol_interner.len(),
            bigints: inner.bigint_interner.len(),
            decimals: inner.decimal_interner.len(),
            chars: inner.char_interner.len(),
        };

        let mut counts: FxHashMap<&'static str, (usize, usize)> = FxHashMap::default();
        for (_, obj, state) in inner.cells.iter() {
            let row = counts.entry(type_name(obj)).or_default();
            match state {
                SlotState::Young | SlotState::Marked => row.0 += 1,
                SlotState::Old | SlotState::Remembered => row.1 += 1,
                SlotState::Free => {}
            }
        }
        let mut histogram: Vec<HistogramRow> = counts
            .into_iter()
            .map(|(type_name, (young, old_gen))| HistogramRow {
                type_name,
                young,
                old_gen,
            })
            .collect();
        histogram.sort_unstable_by_key(|r| std::cmp::Reverse(r.young + r.old_gen));

        GcReport {
            young: young_report,
            old_gen: old_gen_report,
            interners,
            histogram,
        }
    }
}

fn type_name(obj: &HeapObj) -> &'static str {
    obj.tag().name()
}
