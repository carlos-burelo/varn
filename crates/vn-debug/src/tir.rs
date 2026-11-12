use crate::flags::DebugFlags;
use rustc_hash::FxHashMap;
use vn_core::ast::{AstArena, AstId, Program};
use vn_core::term::terminal;
use vn_core::term::terminal::Section;
use vn_sem::bind::BindResult;
use vn_sem::output::{Desugarings, TypeEntry};

pub fn debug_tir(
    program: &Program,
    ast_arena: &AstArena,
    bind: &BindResult,
    expr_table: &FxHashMap<AstId, TypeEntry>,
    call_mappings: &FxHashMap<AstId, Vec<Option<usize>>>,
    desugar: &Desugarings,
    flags: &DebugFlags,
) {
    let module =
        vn_emit::emit_module(program, ast_arena, bind, expr_table, call_mappings, desugar);

    if flags.tir {
        Section::new("tir")
            .subtitle(module.source_file.clone())
            .color(|c| c.magenta())
            .print();
        terminal::log(format!("{module:#?}"));
        Section::new("tir").close();
    }

    if flags.tir_check {
        let source = std::fs::read_to_string(program.filename.as_ref()).unwrap_or_default();
        let lines = vn_compiler::from_tir::compile::line_starts_of(&source);
        Section::new("tir check")
            .subtitle(module.source_file.clone())
            .color(|c| c.magenta())
            .print();
        match vn_tir::verify_module(&module) {
            Ok(()) => {}
            Err(errors) => {
                for e in &errors {
                    terminal::error(format!(
                        "verify error @ {}..{}: {}",
                        e.span.start, e.span.end, e.message
                    ));
                }
                terminal::warn(format!("{} verify error(s)", errors.len()));
            }
        }
        for line in vn_tir::Coverage::of(&module).report().lines() {
            terminal::log(line.to_string());
        }

        match vn_compiler::from_tir::build_module(&module, &lines) {
            Ok(fns) => terminal::info(format!("from_tir(ssa): OK ({} ssa fn(s))", fns.len())),
            Err(e) => terminal::warn(format!("from_tir(ssa): {e:?}")),
        }

        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            vn_compiler::from_tir::compile_module(&module, vec![], &source, false).0
        })) {
            Ok(Ok(_)) => terminal::info("from_tir(proto): OK"),
            Ok(Err(e)) => terminal::warn(format!("from_tir(proto): {e:?}")),
            Err(_) => terminal::error("from_tir(proto): PANIC"),
        }
        Section::new("tir check").close();
    }
}
