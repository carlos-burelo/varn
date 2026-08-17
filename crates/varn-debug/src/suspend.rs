use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_compiler::ssa::ir::SsaFunc;
use varn_compiler::ssa::suspend::{SuspendKind, SuspendPoint};
use varn_core::ast::Program;
use varn_core::TypeAnnotations;

const DIM: &str = "\x1b[2m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const BOLD: &str = "\x1b[1m";
const R: &str = "\x1b[0m";

pub fn debug_suspend(
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
                "\n{BOLD}{BLUE}SUSPEND{R}{DIM} ────────────────────────────── {}{R}",
                program.filename
            );

            if funcs.is_empty() {
                eprintln!(
                    "  {YELLOW}warn{R} No functions lowered to SSA \
                     (coverage is partial — straight-line + if/else only)"
                );
            }

            for func in &funcs {
                let points = varn_compiler::ssa::suspend::analyze(func);
                dump_func(func, &points);
            }

            if !skipped.is_empty() {
                eprintln!(
                    "\n  {DIM}── SSA coverage gaps ({} fn(s) skipped) ──{R}",
                    skipped.len()
                );
                for (name, reason) in &skipped {
                    eprintln!("  {DIM}  {name}: {reason}{R}");
                }
            }

            eprintln!("{DIM}── end: SUSPEND ──{R}");
        }
        Err(varn_compiler::OptError::Unsupported(msg)) => {
            eprintln!("\n  {YELLOW}warn{R} HIR lowering failed — cannot build SSA");
            eprintln!("  {DIM}unsupported construct: {msg}{R}");
        }
    }
}

fn dump_func(func: &SsaFunc, points: &[SuspendPoint]) {
    let mut flags = Vec::new();
    if func.is_async {
        flags.push("async");
    }
    if func.is_generator {
        flags.push("generator");
    }
    if flags.is_empty() {
        eprintln!("  fn {}:", func.name);
    } else {
        eprintln!("  fn {}: [{}]", func.name, flags.join(", "));
    }
    if points.is_empty() {
        eprintln!("    {DIM}(sin puntos de suspensión){R}");
        return;
    }
    for p in points {
        let kind = match p.kind {
            SuspendKind::Await => "Await",
            SuspendKind::Yield => "Yield",
        };
        let dest = match p.dest {
            Some(v) => format!("v{}", v.0),
            None => "-".to_owned(),
        };
        eprintln!(
            "    b{}:i{} {kind} operand=v{} dest={dest} live={} in_try={} in_loop={}",
            p.block,
            p.inst,
            p.operand.0,
            p.live.len(),
            p.in_try,
            p.in_loop,
        );
    }
}
