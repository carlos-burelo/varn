//! Renders `varn_jit::diagnose_loops`' report as part of `vn debug -p
//! bytecode`. Built for agents investigating why a loop isn't as fast as
//! expected: shows every natural loop the JIT sees, whether its
//! array-guard hoisting fast path (see `varn-jit`'s `loop_hoist` module)
//! engages, and — when it doesn't — which specific check rejected it,
//! instead of requiring a manual trace through the JIT's analysis passes.

use varn_core::OpCode;
use varn_jit::CacheSource;
use varn_term::terminal;

const DIM: &str = "\x1b[2m";
const R: &str = "\x1b[0m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";

/// This view runs on bytecode straight out of `emit`, before the
/// global-slot resolution pass (`resolve_globals_in_proto`, run at VM
/// startup) rewrites `LoadGlobal`/`StoreGlobal`/`DefineGlobal` (a
/// name-lookup FFI call) to `LoadGlobalIdx`/`StoreGlobalIdx`/
/// `DefineGlobalIdx` (a single register-indexed read/write, and the ONLY
/// form `varn_jit::is_alloc_free_op` accepts — correctly, since that's the
/// only form the real JIT compiler ever sees). A loop whose only
/// disqualifying instructions are these three is fully alloc-free post-
/// resolution; this scan answers that question so the verdict below can
/// say so instead of reporting a real allocation hazard.
fn alloc_free_ignoring_global_resolution(
    code: &[u16],
    constants: &[varn_types::PoolEntry],
    header_offset: usize,
    latch_offset: usize,
) -> bool {
    let Some(last_len) = varn_types::bytecode::decode(code, latch_offset, constants).map(|i| i.len)
    else {
        return false;
    };
    let end = latch_offset + last_len;
    let mut off = header_offset;
    while off < end {
        let Some(op) = OpCode::from_u16(code[off]) else {
            return false;
        };
        let Some(info) = varn_types::bytecode::decode(code, off, constants) else {
            return false;
        };
        let ok = varn_jit::is_alloc_free_op(op)
            || matches!(
                op,
                OpCode::LoadGlobal | OpCode::StoreGlobal | OpCode::DefineGlobal
            );
        if !ok {
            return false;
        }
        off += info.len;
    }
    true
}

/// `code`/`constants` must be the same slices the bytecode dump above just
/// printed — no separate compile step.
pub fn print_loop_diagnostics(code: &[u16], constants: &[varn_types::PoolEntry], indent: &str) {
    let loops = varn_jit::diagnose_loops(code, constants);
    if loops.is_empty() {
        return;
    }

    terminal::log(format!(
        "{indent}{DIM}array-hoist loop diagnostics ({} loop{}){R}",
        loops.len(),
        if loops.len() == 1 { "" } else { "s" }
    ));

    // Either kind of masking below stems from the same cause (this view
    // predates global-slot resolution) — flag it once at the end rather
    // than repeating the explanation per loop.
    let mut masked_by_resolution = false;

    for lp in &loops {
        let verdict = if !lp.candidates.is_empty() {
            format!("{GREEN}HOISTED{R}")
        } else if !lp.is_real {
            format!("{RED}blocked{R}: header entered by jump, not fallthrough")
        } else if !lp.is_alloc_free {
            if alloc_free_ignoring_global_resolution(
                code,
                constants,
                lp.header_offset,
                lp.latch_offset,
            ) {
                masked_by_resolution = true;
                format!(
                    "{YELLOW}looks blocked here{R}: only disqualifying ops are \
                     LoadGlobal/StoreGlobal/DefineGlobal — alloc-free once resolved"
                )
            } else {
                format!("{RED}blocked{R}: body has an allocating/call-shaped op")
            }
        } else if !lp.is_innermost {
            format!("{DIM}skipped{R}: contains a nested loop (only innermost hoists)")
        } else {
            masked_by_resolution = true;
            format!("{YELLOW}eligible, no invariant array found{R}")
        };

        terminal::log(format!(
            "{indent}  {DIM}@{:04}{R}..{DIM}@{:04}{R}  {}",
            lp.header_offset, lp.latch_offset, verdict
        ));

        for (i, c) in lp.candidates.iter().enumerate() {
            let src = match c.source {
                CacheSource::RegisterInvariant => "register-invariant".to_string(),
                CacheSource::GlobalInvariant(idx) => {
                    format!("global-invariant, global-store idx {idx}")
                }
            };
            terminal::log(format!(
                "{indent}    {DIM}cache_reg[{i}]{R} ← r{} ({src})",
                c.obj_vreg
            ));
        }
    }

    if masked_by_resolution {
        terminal::log(format!(
            "{indent}  {DIM}note: this view compiles bytecode before global-slot resolution{R}"
        ));
        terminal::log(format!(
            "{indent}  {DIM}(LoadGlobal, not LoadGlobalIdx) — a loop reading a top-level{R}"
        ));
        terminal::log(format!(
            "{indent}  {DIM}array via a global can look blocked or show \"no invariant{R}"
        ));
        terminal::log(format!(
            "{indent}  {DIM}array found\" here yet still hoist at runtime. Check{R}"
        ));
        terminal::log(format!(
            "{indent}  {DIM}`vn bench -v`'s JIT stats for what actually ran.{R}"
        ));
    }
}
