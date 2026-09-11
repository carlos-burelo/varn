//! Post-mortem heap/GC snapshot for `vn debug -p gc`.
//!
//! Lives here (not in `varn-debug`, home to every other `-p` phase's
//! formatting) because `varn-debug` deliberately does not depend on
//! `varn-vm` — the Etapa 4 globals rework cut that edge to break a cycle.
//! `Display` is this crate's own concern to own, so `vn debug -p gc` just
//! calls `Vm::gc_report()` and prints it; `crate::heap::Heap::gc_report` is
//! the one place that actually reads `HeapInner`, kept separate so this file
//! stays plain data plus formatting.
//!
//! For watching collections as they HAPPEN rather than a snapshot at exit,
//! see [`crate::gc_trace`] (`VARN_GC_TRACE=1`).

/// Nursery (young generation) counters at the moment the report is taken.
pub struct NurseryReport {
    /// Total slots the nursery ever has (`NURSERY_CAPACITY`), fixed at birth.
    pub capacity: usize,
    /// Fill level at which a minor collection triggers.
    pub full_threshold: usize,
    /// Objects alive in the nursery right now.
    pub live: usize,
    /// Every `try_alloc` that has ever succeeded, this process.
    pub alloc_count: u64,
    /// Minor collections run so far.
    pub minor_gc_count: u64,
    /// Objects promoted to old-gen across every minor collection so far.
    pub minor_gc_promoted: u64,
}

impl NurseryReport {
    /// Objects reclaimed (never promoted, never referenced again) across
    /// every minor collection so far — everything the nursery has ever held
    /// minus what's alive now minus what got promoted.
    pub fn reclaimed(&self) -> u64 {
        self.alloc_count
            .saturating_sub(self.live as u64)
            .saturating_sub(self.minor_gc_promoted)
    }

    /// Fraction of promoted objects that survived to old-gen, of everything
    /// the nursery ever held. High means most allocations are long-lived
    /// (the nursery isn't doing much filtering); low means most are garbage
    /// by the time a collection runs (the common, cheap case).
    pub fn promotion_rate(&self) -> f64 {
        if self.alloc_count == 0 {
            return 0.0;
        }
        self.minor_gc_promoted as f64 / self.alloc_count as f64
    }
}

/// Old generation counters.
pub struct OldGenReport {
    /// Length of the slot table — includes holes freed but not yet reused.
    pub slots_total: usize,
    /// Slots actually holding an object right now.
    pub slots_live: usize,
    /// Freed slots pending reuse — old-gen fragmentation. High relative to
    /// `slots_live` means a lot of the table is holes, not live data.
    pub free_list: usize,
    /// Every direct old-gen allocation (nursery overflow, and anything
    /// allocated straight into old-gen) — NOT nursery allocations later
    /// promoted; see [`NurseryReport::alloc_count`] for those.
    pub alloc_count: u64,
    /// Old-gen collections run so far (rare — most work happens in the
    /// nursery; a program that never fills old-gen shows 0 here, which is
    /// healthy, not a bug).
    pub gc_collections: u64,
    pub gc_total_freed: u64,
    pub gc_alloc_since_collect: u64,
    pub gc_threshold: u64,
}

/// Sizes of the content-addressed interning tables. These only ever grow
/// (nothing evicts them) — a table much larger than expected for the
/// program's actual distinct-value count is a real leak, not the GC's
/// concern but visible right here.
#[derive(Default)]
pub struct InternerSizes {
    pub strings: usize,
    pub symbols: usize,
    pub arrays: usize,
    pub objects: usize,
    pub maps: usize,
    pub sets: usize,
    pub bigints: usize,
    pub decimals: usize,
    pub chars: usize,
}

/// Live-object count by type, nursery and old-gen counted separately — an
/// object that's mostly ending up in one generation or the other is exactly
/// what changes whether it's worth optimizing at all.
pub struct HistogramRow {
    pub type_name: &'static str,
    pub nursery: usize,
    pub old_gen: usize,
}

pub struct GcReport {
    pub nursery: NurseryReport,
    pub old_gen: OldGenReport,
    pub interners: InternerSizes,
    /// Sorted by total (nursery + old_gen) descending.
    pub histogram: Vec<HistogramRow>,
}

impl std::fmt::Display for GcReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = &self.nursery;
        writeln!(f, "gc ─────────────────────────────────────")?;
        writeln!(f, "nursery")?;
        writeln!(
            f,
            "  live={} / capacity={} (minor GC at {})",
            n.live, n.capacity, n.full_threshold
        )?;
        writeln!(
            f,
            "  alloc_count={}  minor_gc_count={}  promoted={} ({:.1}%)  reclaimed={}",
            n.alloc_count,
            n.minor_gc_count,
            n.minor_gc_promoted,
            n.promotion_rate() * 100.0,
            n.reclaimed()
        )?;
        let avg_per_gc = if n.minor_gc_count > 0 {
            n.alloc_count as f64 / n.minor_gc_count as f64
        } else {
            0.0
        };
        writeln!(f, "  avg allocs between minor collections: {avg_per_gc:.0}")?;

        let o = &self.old_gen;
        writeln!(f, "old-gen")?;
        writeln!(
            f,
            "  slots_live={} / slots_total={} (free_list={} — fragmentation if this is large)",
            o.slots_live, o.slots_total, o.free_list
        )?;
        writeln!(
            f,
            "  alloc_count={}  gc_collections={}  gc_total_freed={}  gc_alloc_since_collect={}/{}",
            o.alloc_count,
            o.gc_collections,
            o.gc_total_freed,
            o.gc_alloc_since_collect,
            o.gc_threshold
        )?;

        let i = &self.interners;
        writeln!(
            f,
            "interners  str={} sym={} arr={} obj={} map={} set={} bigint={} decimal={} char={}",
            i.strings,
            i.symbols,
            i.arrays,
            i.objects,
            i.maps,
            i.sets,
            i.bigints,
            i.decimals,
            i.chars
        )?;

        writeln!(f, "live objects by type (nursery / old-gen / total)")?;
        for row in &self.histogram {
            let total = row.nursery + row.old_gen;
            if total == 0 {
                continue;
            }
            writeln!(
                f,
                "  {:<12} {:>8} / {:>8} / {:>8}",
                row.type_name, row.nursery, row.old_gen, total
            )?;
        }
        Ok(())
    }
}
