//! `vn debug -p clif` — Cranelift backend introspection, per function.

#[cfg(target_arch = "x86_64")]
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, IntelFormatter};
use varn_jit::clif::debug::{inspect, ClifInspection};
use varn_jit::clif::lower::NoLinker;
use varn_jit::JitHelpers;
use varn_types::{FunctionProto, PoolEntry};

use crate::flags::DebugFlags;
use crate::walk::constants_for_inspect;

use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_core::term::terminal::Section;

/// Entry point: render the clif views for `proto` and every nested proto.
pub fn debug_clif(proto: &FunctionProto, flags: &DebugFlags, helpers: &JitHelpers) {
    Section::new("clif")
        .subtitle(proto.name.as_deref().unwrap_or("<top-level>"))
        .color(|c| c.blue())
        .print();
    let isa = match varn_jit::clif::shared_isa() {
        Ok(isa) => isa,
        Err(e) => {
            terminal::error(format!("host ISA unavailable: {e}"));
            Section::new("clif").close();
            return;
        }
    };
    // Same shape production lowers — see `crate::resolved_copy`.
    let resolved = crate::resolved_copy(proto);
    render_recursive(&resolved, flags, helpers, isa);
    Section::new("clif").close();
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
    // makes it usable as a sweep over every function in the corpus. With the
    // bytecode lowering gone there are no lowering invariants left to check,
    // so "broken" is a function that refuses to route.
    let check_only = flags.clif_check
        && !(flags.clif_route || flags.clif_kinds || flags.clif_ir || flags.clif_asm);
    if check_only {
        if let Err(reason) = &insp.route {
            terminal::log(format!("  {}", chalk(&insp.name).bold()));
            terminal::error(format!("BAIL  {reason}"));
        }
        return;
    }

    let fa = if insp.frame_aware {
        " (frame-aware)"
    } else {
        ""
    };
    terminal::log(format!(
        "  {}{}",
        chalk(&insp.name).bold(),
        chalk(fa).dim()
    ));

    if flags.clif_check {
        match &insp.route {
            Ok(()) => terminal::info("route ok"),
            Err(reason) => terminal::error(format!("BAIL  {reason}")),
        }
    }

    if flags.clif_route {
        match &insp.route {
            Ok(()) => terminal::info("ROUTE"),
            Err(reason) => terminal::error(format!("BAIL  {reason}")),
        }
    }

    if flags.clif_kinds {
        if let Some(k) = &insp.kinds {
            terminal::log(format!("    {}", chalk(format!("kinds ({} regs):", k.nregs)).dim()));
            let mut table = terminal::Table::new(["block", "kinds"]);
            for (start, ks) in &k.blocks {
                table.row([format!("block@{start}"), ks.join(", ")]);
            }
            table.print();
        }
    }

    if flags.clif_ir {
        if let Some(ir) = &insp.clif_ir {
            terminal::log(format!("    {}", chalk("clif ir:").dim()));
            for line in ir.lines() {
                terminal::log(format!("      {line}"));
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
            terminal::log(format!(
                "    {}",
                chalk(format!("machine code raw@{}:", code.raw_off)).dim()
            ));
            for line in disasm(
                &code.bytes[code.raw_off.min(n)..raw_end],
                code.raw_off as u64,
            )
            .lines()
            {
                terminal::log(line.to_string());
            }
            terminal::log(format!(
                "    {}",
                chalk(format!("machine code wrapper@{}:", code.entry_off)).dim()
            ));
            for line in disasm(&code.bytes[entry..], entry as u64).lines() {
                terminal::log(line.to_string());
            }
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
