pub struct YoungReport {
    pub threshold: usize,

    pub live: usize,

    pub alloc_count: u64,

    pub minor_gc_count: u64,

    pub minor_gc_promoted: u64,
}

impl YoungReport {
    pub fn reclaimed(&self) -> u64 {
        self.alloc_count
            .saturating_sub(self.live as u64)
            .saturating_sub(self.minor_gc_promoted)
    }

    pub fn promotion_rate(&self) -> f64 {
        if self.alloc_count == 0 {
            return 0.0;
        }
        self.minor_gc_promoted as f64 / self.alloc_count as f64
    }
}

pub struct OldGenReport {
    pub slots_total: usize,

    pub slots_live: usize,

    pub free_list: usize,

    pub alloc_count: u64,

    pub gc_collections: u64,
    pub gc_total_freed: u64,
    pub gc_alloc_since_collect: u64,
    pub gc_threshold: u64,
}

#[derive(Default)]
pub struct InternerSizes {
    pub strings: usize,
    pub symbols: usize,
    pub bigints: usize,
    pub decimals: usize,
    pub chars: usize,
}

pub struct HistogramRow {
    pub type_name: &'static str,
    pub young: usize,
    pub old_gen: usize,
}

pub struct GcReport {
    pub young: YoungReport,
    pub old_gen: OldGenReport,
    pub interners: InternerSizes,

    pub histogram: Vec<HistogramRow>,
}

impl std::fmt::Display for GcReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = &self.young;
        writeln!(f, "gc ─────────────────────────────────────")?;
        writeln!(f, "young")?;
        writeln!(f, "  live={} (minor GC at {})", n.live, n.threshold)?;
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
            "interners  str={} sym={} bigint={} decimal={} char={}",
            i.strings, i.symbols, i.bigints, i.decimals, i.chars
        )?;

        writeln!(f, "live objects by type (young / old-gen / total)")?;
        for row in &self.histogram {
            let total = row.young + row.old_gen;
            if total == 0 {
                continue;
            }
            writeln!(
                f,
                "  {:<12} {:>8} / {:>8} / {:>8}",
                row.type_name, row.young, row.old_gen, total
            )?;
        }
        Ok(())
    }
}
