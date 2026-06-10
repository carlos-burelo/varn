use varn_utilities::chalk::chalk;
use varn_utilities::terminal;
use varn_vm::HotspotCounters;

const TOP_N: usize = 12;
const W_NAME: usize = 28;
const W_COUNT: usize = 8;

fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

fn section_header(title: &str) {
    terminal::log(chalk(title).yellow().bold());
    terminal::log(chalk(format!("  {:-<W_NAME$}  {:-<W_COUNT$}", "", "")).dim());
}

fn section_blank() {
    terminal::blank();
}

pub fn print_hotspots(h: &HotspotCounters) {
    terminal::blank();
    terminal::log(chalk("Runtime Hotspots").cyan().bold());

    // Functions
    if !h.fn_calls.is_empty() {
        section_blank();
        section_header("Functions");
        let mut entries: Vec<_> = h.fn_calls.iter().collect();
        entries.sort_by(|a, b| b.1.calls.cmp(&a.1.calls));
        for (name, entry) in entries.iter().take(TOP_N) {
            let jit_pct = if entry.calls > 0 {
                entry.jit_calls * 100 / entry.calls
            } else {
                0
            };
            terminal::log(format!(
                "  {:<W_NAME$}  {:>W_COUNT$}   jit {:>3}%  interp {}",
                name,
                fmt_num(entry.calls),
                jit_pct,
                fmt_num(entry.interp_calls),
            ));
        }
    }

    // Methods
    if !h.method_calls.is_empty() {
        section_blank();
        section_header("Methods");
        let mut entries: Vec<_> = h.method_calls.iter().collect();
        entries.sort_by(|a, b| b.1.calls.cmp(&a.1.calls));
        for (name, entry) in entries.iter().take(TOP_N) {
            let jit_pct = if entry.calls > 0 {
                entry.jit_calls * 100 / entry.calls
            } else {
                0
            };
            terminal::log(format!(
                "  {:<W_NAME$}  {:>W_COUNT$}   jit {:>3}%  interp {}",
                name,
                fmt_num(entry.calls),
                jit_pct,
                fmt_num(entry.interp_calls),
            ));
        }
    }

    // Natives
    if !h.native_calls.is_empty() {
        section_blank();
        section_header("Native Calls");
        let mut entries: Vec<_> = h.native_calls.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));
        for (name, count) in entries.iter().take(TOP_N) {
            terminal::log(format!(
                "  {:<W_NAME$}  {:>W_COUNT$}",
                name,
                fmt_num(**count),
            ));
        }
    }

    // Globals
    if !h.global_accesses.is_empty() {
        section_blank();
        section_header("Global Accesses");
        let mut entries: Vec<_> = h.global_accesses.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));
        for (name, count) in entries.iter().take(TOP_N) {
            terminal::log(format!(
                "  {:<W_NAME$}  {:>W_COUNT$}",
                name,
                fmt_num(**count),
            ));
        }
    }

    // Allocations
    if !h.alloc_types.is_empty() {
        section_blank();
        section_header("Allocation Types");
        let mut entries: Vec<_> = h.alloc_types.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));
        for (type_name, count) in entries.iter().take(TOP_N) {
            terminal::log(format!(
                "  {:<W_NAME$}  {:>W_COUNT$}",
                type_name,
                fmt_num(**count),
            ));
        }
    }
}
