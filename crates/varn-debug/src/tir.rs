//! `-p tir` / `-p tir:check` — the typed IR the checker emits (stage 2).
//!
//! `tir` dumps the module. `tir:check` runs the verifier and prints the
//! coverage report; silence from the verifier is the healthy answer, the
//! coverage line always prints. Neither is part of `-p all`: they sweep a
//! module, they do not read one function.

use crate::flags::DebugFlags;
use rustc_hash::FxHashMap;
use varn_checker::module_resolver::ImportResolver;
use varn_checker::{BindResult, TypeEntry};
use varn_core::ast::{AstId, Program};

pub fn debug_tir(
    program: &Program,
    bind: &BindResult,
    resolver: &dyn ImportResolver,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    flags: &DebugFlags,
) {
    let module = varn_checker::emit::emit_module(program, bind, resolver, expr_table);

    if flags.tir {
        eprintln!("\n=== TIR: {} ===", module.source_file);
        eprintln!("{module:#?}");
    }

    if flags.tir_check {
        eprintln!("\n=== TIR CHECK: {} ===", module.source_file);
        match varn_tir::verify_module(&module) {
            Ok(()) => {}
            Err(errors) => {
                for e in &errors {
                    eprintln!(
                        "  verify error @ {}..{}: {}",
                        e.span.start, e.span.end, e.message
                    );
                }
                eprintln!("  {} verify error(s)", errors.len());
            }
        }
        let coverage = varn_tir::Coverage::of(&module);
        eprint!("{}", coverage.report());
    }
}
