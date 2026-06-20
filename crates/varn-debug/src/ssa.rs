//! `vn debug -p ssa` — dump del SSA CFG generado por `varn-opt`.

use varn_core::ast::Program;
use varn_core::TypeAnnotations;
use rustc_hash::FxHashMap;
use std::rc::Rc;

const DIM: &str = "\x1b[2m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const BOLD: &str = "\x1b[1m";
const R: &str = "\x1b[0m";

/// Construye el SSA CFG para cada función del módulo e imprime el dump.
/// Funciones cuya cobertura SSA falla (construcciones no implementadas aún)
/// se omiten con un aviso; los errores completos de lowering (HIR) producen
/// un mensaje de falla claro.
pub fn debug_ssa(
    program: &Program,
    annotations: &TypeAnnotations,
    extension_calls: &FxHashMap<u32, Rc<str>>,
    extension_members: &FxHashMap<u32, Rc<str>>,
    extension_set_members: &FxHashMap<u32, Rc<str>>,
) {
    let input = varn_opt::OptInput {
        program,
        annotations,
        extension_calls,
        extension_members,
        extension_set_members,
        export_names: Vec::new(),
    };

    match varn_opt::lower_to_ssa(input) {
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

            for func in &funcs {
                let dump = varn_opt::ssa::dump::dump(func);
                eprint!("{dump}");
            }

            if !skipped.is_empty() {
                eprintln!("\n  {DIM}── SSA coverage gaps ({} fn(s) skipped) ──{R}", skipped.len());
                for (name, reason) in &skipped {
                    eprintln!("  {DIM}  {name}: {reason}{R}");
                }
            }

            eprintln!("{DIM}── end: SSA ──{R}");
        }
        Err(varn_opt::OptError::Unsupported(msg)) => {
            eprintln!(
                "\n  {YELLOW}warn{R} HIR lowering failed — cannot build SSA"
            );
            eprintln!("  {DIM}unsupported construct: {msg}{R}");
        }
    }
}
