use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::Program;
use varn_core::TypeAnnotations;

const DIM: &str = "\x1b[2m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const BOLD: &str = "\x1b[1m";
const R: &str = "\x1b[0m";

pub fn debug_ssa(
    program: &Program,
    annotations: &TypeAnnotations,
    extension_calls: &FxHashMap<u32, Rc<str>>,
    extension_members: &FxHashMap<u32, Rc<str>>,
    extension_set_members: &FxHashMap<u32, Rc<str>>,
) {
    let input = varn_compiler::OptInput {
        program,
        annotations,
        extension_calls,
        extension_members,
        extension_set_members,
        export_names: Vec::new(),
    };

    match varn_compiler::lower_to_ssa(input) {
        Ok((funcs, skipped)) => {
            eprintln!(
                "\n{BOLD}{BLUE}SSA{R}{DIM} ─────────────────────────────── {}{R}",
                program.filename
            );

            if funcs.is_empty() {
                eprintln!(
                    "  {YELLOW}warn{R} No functions lowered to SSA \
                     (coverage is partial — straight-line + if/else only)"
                );
            }

            let mut total_typed = 0usize;
            let mut total_dynamic = 0usize;
            for func in &funcs {
                for vdef in &func.values {
                    match vdef.ty {
                        varn_compiler::hir::HirType::Dynamic => total_dynamic += 1,
                        _ => total_typed += 1,
                    }
                }
                let dump = varn_compiler::ssa::dump::dump(func);
                eprint!("{dump}");
            }

            let total = total_typed + total_dynamic;
            let pct = if total > 0 {
                (total_dynamic as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            eprintln!(
                "\n  {DIM}── SSA type coverage: typed={total_typed} dynamic={total_dynamic} ({pct:.1}% dynamic) ──{R}"
            );

            if !skipped.is_empty() {
                eprintln!(
                    "\n  {DIM}── SSA coverage gaps ({} fn(s) skipped) ──{R}",
                    skipped.len()
                );
                for (name, reason) in &skipped {
                    eprintln!("  {DIM}  {name}: {reason}{R}");
                }
            }

            eprintln!("{DIM}── end: SSA ──{R}");
        }
        Err(varn_compiler::OptError::Unsupported(msg)) => {
            eprintln!("\n  {YELLOW}warn{R} HIR lowering failed — cannot build SSA");
            eprintln!("  {DIM}unsupported construct: {msg}{R}");
        }
    }
}
