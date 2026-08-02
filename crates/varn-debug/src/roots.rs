//! `vn debug -p roots` — the GC root set at every safepoint, answered twice.
//!
//! A missing GC root is the worst bug shape this compiler can produce: the
//! object is freed while live, the damage surfaces far from the cause, and it
//! only happens when a collection lands on that exact safepoint. `vn bench`
//! reports 31k nursery allocations against 3 collections for `tests/main.vn`,
//! so a run exercises almost no safepoint at all — passing the suite says
//! nearly nothing about whether the root sets are right.
//!
//! This pass answers statically instead, and so covers EVERY safepoint in the
//! program whether or not it ever executes.
//!
//! **The two columns are complements, not two opinions of one set.** Marking
//! the register variables with `declare_var_needs_stack_map` makes Cranelift
//! record those live ACROSS each call. A register we flush is stored before
//! the call and redefined from its home slot after, so it is dead at the call
//! and never enters a map; only the registers we do NOT flush cross it. The
//! check that fell out of building this: with the flush set forced back to
//! every register (`VARN_JIT_NO_LIVENESS=1`), Cranelift emits zero stack maps
//! for the whole program. Reading the columns as two answers to one question
//! and subtracting them is therefore meaningless — an earlier draft of this
//! pass did exactly that and reported healthy code as losing roots.
//!
//! So the invariant is coverage by union: a register that can hold a live heap
//! reference at a safepoint has to be reachable through the home slot OR
//! through the stack map.
//!
//! * `home` — flushed to `ctx.stack`. What the collector can see today.
//! * `registro` — live across the call and NOT flushed. Invisible to today's
//!   collector, which is sound only because nothing under a bare raw can
//!   allocate; it is exactly the set stack maps would have to root after the
//!   cutover, so it sizes that work.
//! * `unboxed` — live and deliberately not flushed because the kind flow proved
//!   the register holds a raw machine integer. Cranelift marks every I64
//!   Variable and cannot see our kinds, so these show up inside its map count;
//!   they are netted out of `registro` before it is reported. Rooting them
//!   would be meaningless — there is no heap index to rewrite.
//!
//! Counts, never identities: Cranelift owns spill-slot assignment and does not
//! publish which slot holds which variable. `unboxed` is the one exception —
//! it comes from our own kind flow, so the identities are known.

use varn_jit::clif::debug::{inspect_roots, ClifInspection};
use varn_jit::clif::lower::NoLinker;
use varn_jit::{JitHelpers, SIZE_GATE_WORDS};
use varn_types::{FunctionProto, PoolEntry};

use crate::flags::DebugFlags;
use crate::tiers::constants_for_inspect;

const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const R: &str = "\x1b[0m";

struct FnRoots {
    name: String,
    fa_reasons: Vec<&'static str>,
    insp: ClifInspection,
}

fn walk(
    proto: &FunctionProto,
    helpers: &JitHelpers,
    isa: &varn_jit::OwnedTargetIsa,
    out: &mut Vec<FnRoots>,
) {
    // Mirror production's order: the size gate fires before Cranelift is
    // asked, so a gated function has no safepoints to report.
    if proto.chunk.code.len() <= SIZE_GATE_WORDS {
        let constants = constants_for_inspect(proto);
        let insp = inspect_roots(proto, &constants, helpers, isa, &NoLinker);
        if insp.roots.is_some() {
            out.push(FnRoots {
                name: proto.name.as_deref().unwrap_or("<module>").to_owned(),
                fa_reasons: insp.fa_reasons.clone(),
                insp,
            });
        }
    }
    for entry in &proto.chunk.constants {
        if let PoolEntry::Function(f) = entry {
            walk(f, helpers, isa, out);
        }
    }
}

/// `header` labels the module and prints only when there is something to show,
/// so a `--fn` filter leaves no trail of empty banners.
pub fn debug_roots(
    proto: &FunctionProto,
    flags: &DebugFlags,
    helpers: &JitHelpers,
    header: Option<&str>,
) {
    let Ok(isa) = varn_jit::clif::shared_isa() else {
        return;
    };
    let mut fns = Vec::new();
    walk(proto, helpers, isa, &mut fns);
    if let Some(needle) = &flags.fn_filter {
        fns.retain(|f| f.name.contains(needle.as_str()));
    }

    let mut safepoints = 0usize;
    let mut fully_flushed = 0usize;
    let mut with_reg_roots = 0usize;
    let mut reg_roots_total = 0usize;
    let mut reg_roots_max = 0usize;
    let mut unboxed_total = 0usize;
    let mut unmatched = 0usize;
    let mut rendered = Vec::new();

    for f in &fns {
        let Some(rep) = &f.insp.roots else { continue };
        unmatched += rep.maps_unmatched;
        let mut rows = Vec::new();
        for p in &rep.points {
            safepoints += 1;
            // Net out the registers the kind flow proved unboxed: Cranelift
            // counted them because it marks every I64 Variable, but they carry
            // no heap index and are not roots the cutover would have to cover.
            let in_reg = p.cranelift.unwrap_or(0).saturating_sub(p.unboxed.len());
            unboxed_total += p.unboxed.len();
            if in_reg == 0 {
                fully_flushed += 1;
            } else {
                with_reg_roots += 1;
                reg_roots_total += in_reg;
                reg_roots_max = reg_roots_max.max(in_reg);
            }
            // `roots:diff` = only where the two models differ, i.e. where a
            // live value sits somewhere the home slots do not describe.
            if flags.roots_diff && in_reg == 0 {
                continue;
            }
            rows.push((p, in_reg));
        }
        if !rows.is_empty() && !flags.roots_summary {
            rendered.push((f, rows));
        }
    }

    if safepoints == 0 {
        return;
    }
    if let Some(h) = header {
        eprintln!("\n{DIM}=== {h} ==={R}");
    }

    for (f, rows) in rendered {
        let fa = if f.fa_reasons.is_empty() {
            String::new()
        } else {
            format!("  (frame-aware: {})", f.fa_reasons.join("+"))
        };
        eprintln!("\n{BOLD}ROOTS{R}{DIM} ── {}{fa}{R}", f.name);
        eprintln!(
            "  {DIM}{:>6}  {:<18} {:>5}  {:>8}  {:>8}   home slots{R}",
            "ip", "opcode", "home", "registro", "unboxed"
        );
        for (p, in_reg) in rows {
            let colour = if in_reg == 0 { DIM } else { YELLOW };
            let regs: Vec<String> = p.ours.iter().map(|r| format!("r{r}")).collect();
            let un: Vec<String> = p.unboxed.iter().map(|r| format!("r{r}")).collect();
            eprintln!(
                "  {:>6}  {:<18} {:>5}  {colour}{:>8}{R}  {DIM}{:>8}{R}   {DIM}{}{R}",
                p.ip,
                p.op,
                p.ours.len(),
                in_reg,
                un.join(" "),
                regs.join(" ")
            );
        }
    }

    eprintln!(
        "\n  {GREEN}{safepoints} safepoints{R}{DIM} · {fully_flushed} cubiertos solo por \
         home slots · {with_reg_roots} con valores vivos en registro{R}"
    );
    if unboxed_total > 0 {
        eprintln!(
            "  {DIM}{unboxed_total} registros vivos sin flushear por ser Int/Bool — no son \
             raíces, no entran en la cuenta de arriba.{R}"
        );
    }
    if with_reg_roots > 0 {
        eprintln!(
            "  {YELLOW}{reg_roots_total} raíces en registro{R}{DIM} (máx {reg_roots_max} en un \
             safepoint) — lo que los stack maps tendrían que rootear tras el cutover.{R}"
        );
        eprintln!(
            "  {DIM}Hoy es sano porque nada bajo un raw sin frame puede alocar; deja de serlo \
             en cuanto `has_alloc` salga de `frame_aware`.{R}"
        );
    }
    if unmatched > 0 {
        eprintln!(
            "  {RED}✗ {unmatched} stack maps sin correlacionar{R}\
             {DIM} — su PC no cayó tras ningún srcloc; la unión ip↔offset está incompleta{R}"
        );
    }
}
