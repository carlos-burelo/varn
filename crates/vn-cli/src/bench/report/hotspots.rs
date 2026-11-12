use vn_vm::HotspotCounters;

use super::fmt::{share, short_global};
use super::plain::{
    Align::{Left, Right},
    Report, Table,
};

const TOP_N: usize = 20;

pub fn hotspots(report: &mut Report, h: &HotspotCounters) {
    if !h.fn_calls.is_empty() {
        let mut entries: Vec<_> = h.fn_calls.iter().collect();
        entries.sort_by_key(|(name, e)| (std::cmp::Reverse(e.calls), name.to_string()));
        report.title("functions by calls");
        let mut table = Table::new(
            ["function", "calls", "clif%", "interp"],
            [Left, Right, Right, Right],
        );
        for (name, entry) in entries.into_iter().take(TOP_N) {
            table.row([
                name.to_string(),
                entry.calls.to_string(),
                share(entry.jit_calls, entry.calls),
                entry.interp_calls.to_string(),
            ]);
        }
        report.table(&table);
    }

    if !h.method_calls.is_empty() {
        let mut entries: Vec<_> = h.method_calls.iter().collect();
        entries.sort_by_key(|(name, e)| (std::cmp::Reverse(e.calls), name.to_string()));
        report.title("methods by calls");
        let mut table = Table::new(["method", "calls", "clif%"], [Left, Right, Right]);
        for (name, entry) in entries.into_iter().take(TOP_N) {
            table.row([
                name.to_string(),
                entry.calls.to_string(),
                share(entry.jit_calls, entry.calls),
            ]);
        }
        report.table(&table);
    }

    if !h.native_calls.is_empty() {
        let mut entries: Vec<_> = h.native_calls.iter().collect();
        entries.sort_by_key(|(name, count)| {
            (
                std::cmp::Reverse(h.native_ns.get(*name).copied().unwrap_or(0).max(**count)),
                name.to_string(),
            )
        });
        report.title("native calls");
        let mut table = Table::new(
            ["native", "calls", "total[ms]", "ns/call"],
            [Left, Right, Right, Right],
        );
        for (name, count) in entries.into_iter().take(TOP_N) {
            let ns = h.native_ns.get(name).copied().unwrap_or(0);
            table.row([
                name.to_string(),
                count.to_string(),
                format!("{:.3}", ns as f64 / 1_000_000.0),
                (ns / (*count).max(1)).to_string(),
            ]);
        }
        report.table(&table);
    }

    if !h.global_accesses.is_empty() {
        let mut entries: Vec<_> = h.global_accesses.iter().collect();
        entries.sort_by_key(|(name, count)| (std::cmp::Reverse(**count), name.to_string()));
        report.title("global accesses");
        let mut table = Table::new(["global", "count"], [Left, Right]);
        for (name, count) in entries.into_iter().take(TOP_N) {
            table.row([short_global(name), count.to_string()]);
        }
        report.table(&table);
    }

    if !h.alloc_types.is_empty() {
        let mut entries: Vec<_> = h.alloc_types.iter().collect();
        entries.sort_by_key(|(name, count)| (std::cmp::Reverse(**count), name.to_string()));
        let total: u64 = entries.iter().map(|(_, count)| **count).sum();
        report.title("allocations by type");
        let mut table = Table::new(["type", "count", "share%"], [Left, Right, Right]);
        for (name, count) in entries.into_iter().take(TOP_N) {
            table.row([name.to_string(), count.to_string(), share(*count, total)]);
        }
        report.table(&table);
    }
}
