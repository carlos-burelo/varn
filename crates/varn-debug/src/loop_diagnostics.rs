






use varn_core::term::terminal;
use varn_core::OpCode;
use varn_jit::CacheSource;

use varn_core::term::colors::{DIM, GREEN, R, RED, YELLOW};






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
