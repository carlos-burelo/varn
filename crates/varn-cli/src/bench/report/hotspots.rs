//! Runtime hotspots: functions, natives, globals, allocations.

use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_vm::HotspotCounters;

use super::fmt::{fmt_num, fmt_pct, row, row_note, short_global, LABEL_WIDTH};

const TOP_N: usize = 12;

fn section(title: &str) {
    terminal::blank();
    terminal::log(format!("  {}", chalk(title).yellow().bold()));
}

pub fn print_hotspots(h: &HotspotCounters) {
    terminal::blank();
    terminal::log(chalk("Runtime Hotspots").cyan().bold());

    if !h.fn_calls.is_empty() {
        section("Funciones");
        let mut entries: Vec<_> = h.fn_calls.iter().collect();
        entries.sort_by(|a, b| b.1.calls.cmp(&a.1.calls));
        let counted: u64 = entries.iter().map(|(_, e)| e.calls).sum();
        for (name, entry) in entries.iter().take(TOP_N) {
            let jit_share = if entry.calls > 0 {
                entry.jit_calls as f64 / entry.calls as f64
            } else {
                0.0
            };
            let note = if entry.interp_calls > 0 {
                chalk(format!(
                    "clif {}  interp {}",
                    fmt_pct(jit_share),
                    fmt_num(entry.interp_calls)
                ))
                .red()
                .to_string()
            } else {
                chalk(format!("clif {}", fmt_pct(jit_share)))
                    .dim()
                    .to_string()
            };
            terminal::log(format!("{}  {note}", row(name, fmt_num(entry.calls))));
        }
        terminal::log(row_note(
            "total contabilizado",
            fmt_num(counted),
            "solo llamadas con nombre",
        ));
    }

    if !h.method_calls.is_empty() {
        section("Métodos");
        let mut entries: Vec<_> = h.method_calls.iter().collect();
        entries.sort_by(|a, b| b.1.calls.cmp(&a.1.calls));
        for (name, entry) in entries.iter().take(TOP_N) {
            let jit_share = if entry.calls > 0 {
                entry.jit_calls as f64 / entry.calls as f64
            } else {
                0.0
            };
            terminal::log(row_note(
                name,
                fmt_num(entry.calls),
                format!("clif {}", fmt_pct(jit_share)),
            ));
        }
    }

    if !h.native_calls.is_empty() {
        section("Llamadas nativas");
        let mut entries: Vec<_> = h.native_calls.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));
        for (name, count) in entries.iter().take(TOP_N) {
            terminal::log(row(name, fmt_num(**count)));
        }
        if h.total_native_ns > 0 {
            terminal::log(row(
                "tiempo nativo total",
                format!("{:.3} ms", h.total_native_ns as f64 / 1_000_000.0),
            ));
        }
    }

    if !h.global_accesses.is_empty() {
        section("Accesos a globals");
        let mut entries: Vec<_> = h.global_accesses.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));
        for (name, count) in entries.iter().take(TOP_N) {
            // Globals carry their defining module as an absolute path; printed
            // raw they are ~70 chars against a 26-wide column and shear the
            // whole block apart.
            terminal::log(row(&short_global(name), fmt_num(**count)));
        }
    }

    if !h.alloc_types.is_empty() {
        section("Tipos alocados");
        let mut entries: Vec<_> = h.alloc_types.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));
        let counted: u64 = entries.iter().map(|(_, c)| **c).sum();
        for (type_name, count) in entries.iter().take(TOP_N) {
            terminal::log(row(type_name, fmt_num(**count)));
        }
        if entries.len() > TOP_N {
            let shown: u64 = entries.iter().take(TOP_N).map(|(_, c)| **c).sum();
            terminal::log(row(
                &format!("… {} tipos más", entries.len() - TOP_N),
                fmt_num(counted - shown),
            ));
        }
        terminal::log(format!("  {}", chalk("─".repeat(LABEL_WIDTH + 11)).dim()));
        terminal::log(row_note(
            "total por tipo",
            fmt_num(counted),
            "compárese con heap allocs arriba",
        ));
    }
}
