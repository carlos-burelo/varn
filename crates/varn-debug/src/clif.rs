//! `vn debug -p clif` — Cranelift backend introspection, per function.

#[cfg(target_arch = "x86_64")]
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, IntelFormatter};
use varn_jit::clif::debug::{inspect, ClifInspection};
use varn_jit::clif::lower::NoLinker;
use varn_jit::JitHelpers;
use varn_types::{FunctionProto, PoolEntry};

use crate::flags::DebugFlags;
use crate::walk::constants_for_inspect;

const BOLD: &str = "\x1b[1m";
const BLUE: &str = "\x1b[34m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const R: &str = "\x1b[0m";

/// Entry point: render the clif views for `proto` and every nested proto.
pub fn debug_clif(proto: &FunctionProto, flags: &DebugFlags, helpers: &JitHelpers) {
    eprintln!(
        "\n{BOLD}{BLUE}CLIF{R}{DIM} ─────────────────────────────── {}{R}",
        proto.name.as_deref().unwrap_or("<top-level>")
    );
    let isa = match varn_jit::clif::shared_isa() {
        Ok(isa) => isa,
        Err(e) => {
            eprintln!("  {RED}error{R} host ISA unavailable: {e}");
            return;
        }
    };
    // Same shape production lowers — see `crate::resolved_copy`.
    let resolved = crate::resolved_copy(proto);
    render_recursive(&resolved, flags, helpers, isa);
    eprintln!("{DIM}── end: CLIF ──{R}");
}

fn render_recursive(
    proto: &FunctionProto,
    flags: &DebugFlags,
    helpers: &JitHelpers,
    isa: &varn_jit::OwnedTargetIsa,
) {
    // The filter selects which functions are *rendered*, never which are
    // walked: a match can be nested inside a function that does not match.
    if flags.fn_filter.as_ref().is_none_or(|needle| {
        proto
            .name
            .as_deref()
            .unwrap_or("<module>")
            .contains(needle.as_str())
    }) {
        let constants = constants_for_inspect(proto);
        let insp = inspect(proto, &constants, helpers, isa, &NoLinker);
        render_one(&insp, flags);
    }
    for entry in &proto.chunk.constants {
        if let PoolEntry::Function(f) = entry {
            render_recursive(f, flags, helpers, isa);
        }
    }
}

fn render_one(insp: &ClifInspection, flags: &DebugFlags) {
    // Asked for `clif:check` alone, the phase reports only what is broken and a
    // clean module prints nothing — the same contract as `-p bails`, and what
    // makes it usable as a sweep over every function in the corpus.
    let check_only = flags.clif_check
        && !(flags.clif_route || flags.clif_kinds || flags.clif_ir || flags.clif_asm);
    if check_only {
        if !insp.invariants.is_empty() {
            eprintln!("\n  {BOLD}{}{R}", insp.name);
            for v in &insp.invariants {
                eprintln!("    {RED}{}{R}  {}", v.rule, v.detail);
            }
        }
        return;
    }

    let fa = if insp.frame_aware {
        " (frame-aware)"
    } else {
        ""
    };
    eprintln!("\n  {BOLD}{}{R}{DIM}{fa}{R}", insp.name);

    if flags.clif_check {
        if insp.invariants.is_empty() {
            eprintln!("    {GREEN}invariants ok{R}");
        } else {
            for v in &insp.invariants {
                eprintln!("    {RED}{}{R}  {}", v.rule, v.detail);
            }
        }
    }

    if flags.clif_route {
        match &insp.route {
            Ok(()) => eprintln!("    {GREEN}ROUTE{R}"),
            Err(reason) => eprintln!("    {RED}BAIL{R}  {reason}"),
        }
    }

    if flags.clif_kinds {
        if let Some(k) = &insp.kinds {
            eprintln!("    {DIM}kinds ({} regs):{R}", k.nregs);
            for (start, ks) in &k.blocks {
                eprintln!("      block@{start}: [{}]", ks.join(", "));
            }
        }
    }

    if flags.clif_ir {
        if let Some(ir) = &insp.clif_ir {
            eprintln!("    {DIM}clif ir:{R}");
            for line in ir.lines() {
                eprintln!("      {line}");
            }
        }
    }

    if flags.clif_asm {
        if let Some(code) = &insp.code {
            // Decode the raw fn and the ABI wrapper in two independent passes.
            // They are separate code ranges with alignment padding between
            // them; decoding the whole buffer linearly lets the padding
            // desync the decoder and corrupt the wrapper's instructions.
            let n = code.bytes.len();
            let raw_end = (code.raw_off + code.raw_len).min(n);
            let entry = code.entry_off.min(n);
            eprintln!("    {DIM}machine code raw@{}:{R}", code.raw_off);
            eprint!(
                "{}",
                disasm(
                    &code.bytes[code.raw_off.min(n)..raw_end],
                    code.raw_off as u64
                )
            );
            eprintln!("    {DIM}machine code wrapper@{}:{R}", code.entry_off);
            eprint!("{}", disasm(&code.bytes[entry..], entry as u64));
        }
    }
}

/// Decode `bytes` (x86-64) into Intel-syntax text, one instruction per line.
#[cfg(target_arch = "x86_64")]
fn disasm(bytes: &[u8], rip: u64) -> String {
    let mut decoder = Decoder::with_ip(64, bytes, rip, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    let mut out = String::new();
    let mut line = String::new();
    let mut inst = Instruction::default();
    while decoder.can_decode() {
        decoder.decode_out(&mut inst);
        line.clear();
        formatter.format(&inst, &mut line);
        out.push_str(&format!("      {:016x}  {line}\n", inst.ip()));
    }
    out
}

/// Portable byte dump for non-x86 architectures.
#[cfg(not(target_arch = "x86_64"))]
fn disasm(bytes: &[u8], rip: u64) -> String {
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let addr = rip + (i * 16) as u64;
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        out.push_str(&format!("      {:016x}  {}\n", addr, hex.join(" ")));
    }
    out
}
