//! Inspection views backed by the LSP analysis (`-p types`, `-p lsp`).
//!
//! These live in the CLI, not in `varn-pipeline`: they need `varn-lsp`, and an
//! orchestrator that depends on the editor server is an inverted edge. The
//! views only need a path and its source text, so nothing about the pipeline
//! phases is involved -- the CLI runs `varn_lsp::pipeline::run_pipeline`
//! itself.

mod dashboard;
mod types;

use varn_debug::flags::DebugFlags;

/// Print the views selected in `flags`, reading the source from `path` unless
/// the CLI was given inline code. A no-op when no LSP-backed view is selected,
/// so both `vn debug` and `vn inspect` can call it unconditionally.
pub fn run_for(path: &str, eval: Option<&str>, flags: &DebugFlags) {
    if !(flags.types || flags.lsp) {
        return;
    }
    let source = match eval {
        Some(code) => code.to_owned(),
        None => std::fs::read_to_string(path).unwrap_or_default(),
    };
    if flags.types {
        types::debug_types(path, &source, flags);
    }
    if flags.lsp {
        dashboard::debug_lsp(path, &source, flags);
    }
}
