//! `vn debug -p clif` — Cranelift backend introspection, per function.

use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, IntelFormatter};
use varn_jit::clif::debug::{inspect, ClifInspection};
use varn_jit::clif::lower::NoLinker;
use varn_jit::JitHelpers;
use varn_types::{FunctionProto, Literal, PoolEntry, VmValue};

use crate::flags::DebugFlags;

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
    render_recursive(proto, flags, helpers, isa);
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

/// Heap-free constant resolution for inspection. Only `is_int()` fidelity
/// matters to the lowering's kind classification, so scalar literals map
/// exactly (mirroring `varn_vm::exec::calls::resolve_constants`) and heap
/// literals (strings, bigints, symbols, chars) plus function/shape entries
/// become `null` placeholders — they are non-int, which is the correct kind,
/// and their real heap bits are irrelevant to a static, non-executing view.
/// (Consequence: a string constant shows as `null` in the IR/disasm — see
/// the phase limitations.)
fn constants_for_inspect(proto: &FunctionProto) -> Vec<VmValue> {
    const I48_MIN: i64 = -(1_i64 << 47);
    const I48_MAX: i64 = (1_i64 << 47) - 1;
    proto
        .chunk
        .constants
        .iter()
        .map(|entry| match entry {
            PoolEntry::Literal(Literal::Null) => VmValue::null(),
            PoolEntry::Literal(Literal::Bool(b)) => VmValue::from_bool(*b),
            PoolEntry::Literal(Literal::Int(n)) if *n >= I48_MIN && *n <= I48_MAX => {
                VmValue::from_int(*n)
            }
            PoolEntry::Literal(Literal::Int(n)) => VmValue::from_f64(*n as f64),
            PoolEntry::Literal(Literal::Float(f)) => VmValue::from_f64(*f),
            // Heap literals + function/shape entries: non-int placeholder.
            _ => VmValue::null(),
        })
        .collect()
}

fn render_one(insp: &ClifInspection, flags: &DebugFlags) {
    let fa = if insp.frame_aware {
        " (frame-aware)"
    } else {
        ""
    };
    eprintln!("\n  {BOLD}{}{R}{DIM}{fa}{R}", insp.name);

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
            eprintln!("    {DIM}x86-64 raw@{}:{R}", code.raw_off);
            eprint!(
                "{}",
                disasm(
                    &code.bytes[code.raw_off.min(n)..raw_end],
                    code.raw_off as u64
                )
            );
            eprintln!("    {DIM}x86-64 wrapper@{}:{R}", code.entry_off);
            eprint!("{}", disasm(&code.bytes[entry..], entry as u64));
        }
    }
}

/// Decode `bytes` (x86-64) into Intel-syntax text, one instruction per line.
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
