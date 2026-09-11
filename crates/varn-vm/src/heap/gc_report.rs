//! Builds [`crate::gc_report::GcReport`] by actually reading `HeapInner` —
//! split from `crate::gc_report` (the plain-data shape) because this is the
//! one file that needs `pub(super)` access to the heap's private tables.

use super::obj::HeapObj;
use super::structs::Heap;
use crate::gc_report::{GcReport, HistogramRow, InternerSizes, NurseryReport, OldGenReport};
use crate::nursery::Nursery;
use rustc_hash::FxHashMap;

impl Heap {
    pub fn gc_report(&self) -> GcReport {
        let inner = unsafe { &*self.inner.get() };
        let nursery = &inner.nursery;

        let nursery_report = NurseryReport {
            capacity: crate::nursery::NURSERY_CAPACITY,
            full_threshold: Nursery::FULL_THRESHOLD,
            live: nursery.len(),
            alloc_count: nursery.alloc_count,
            minor_gc_count: nursery.minor_gc_count,
            minor_gc_promoted: nursery.minor_gc_promoted,
        };

        let old_gen_report = OldGenReport {
            slots_total: inner.objects.len(),
            slots_live: inner.objects.iter().filter(|o| o.is_some()).count(),
            free_list: inner.free.len(),
            alloc_count: inner.alloc_count,
            gc_collections: inner.gc_collections,
            gc_total_freed: inner.gc_total_freed,
            gc_alloc_since_collect: inner.gc_alloc_since_collect,
            gc_threshold: inner.gc_threshold,
        };

        let interners = InternerSizes {
            strings: inner.string_interner.len(),
            symbols: inner.symbol_interner.len(),
            arrays: inner.array_interner.len(),
            objects: inner.object_interner.len(),
            maps: inner.map_interner.len(),
            sets: inner.set_interner.len(),
            bigints: inner.bigint_interner.len(),
            decimals: inner.decimal_interner.len(),
            chars: inner.char_interner.len(),
        };

        let mut counts: FxHashMap<&'static str, (usize, usize)> = FxHashMap::default();
        for obj in nursery.iter() {
            counts.entry(type_name(obj)).or_default().0 += 1;
        }
        for obj in inner.objects.iter().flatten() {
            counts.entry(type_name(obj)).or_default().1 += 1;
        }
        let mut histogram: Vec<HistogramRow> = counts
            .into_iter()
            .map(|(type_name, (nursery, old_gen))| HistogramRow {
                type_name,
                nursery,
                old_gen,
            })
            .collect();
        histogram.sort_unstable_by_key(|r| std::cmp::Reverse(r.nursery + r.old_gen));

        GcReport {
            nursery: nursery_report,
            old_gen: old_gen_report,
            interners,
            histogram,
        }
    }
}

fn type_name(obj: &HeapObj) -> &'static str {
    obj.tag().name()
}
