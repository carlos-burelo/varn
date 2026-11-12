#[cfg(target_arch = "x86_64")]
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, IntelFormatter};
use vn_jit::clif::debug::{inspect, ClifInspection, CodeBytes};
use vn_jit::clif::lower::gate_reason;
use vn_jit::JitHelpers;
use vn_types::{FunctionProto, PoolEntry};

use crate::tiers::Tier;
use crate::walk::constants_for_inspect;

pub struct CompiledFunction {
    pub name: String,
    pub tier: Tier,
    pub inspection: Option<ClifInspection>,
    lines: Vec<u32>,
}

impl CompiledFunction {
    fn source_line(&self, ip: u32) -> Option<u32> {
        self.lines
            .get(ip as usize)
            .copied()
            .filter(|line| *line > 0)
    }

    fn describe(&self) -> String {
        match (&self.tier, &self.inspection) {
            (Tier::Clif, Some(inspection)) => {
                let activation = if inspection.framed {
                    "native, framed"
                } else if inspection.frame_aware {
                    "native, frame aware"
                } else {
                    "native"
                };
                activation.to_owned()
            }
            (Tier::Clif, None) => "native".to_owned(),
            (Tier::Gate(reason), _) => format!("interpreted, gate: {reason}"),
            (Tier::Bail(reason), _) => format!("interpreted, bail: {reason}"),
        }
    }
}

pub fn collect(proto: &FunctionProto, helpers: &JitHelpers) -> Vec<CompiledFunction> {
    let Ok(isa) = vn_jit::clif::shared_isa() else {
        return Vec::new();
    };
    let mut functions = Vec::new();
    walk(proto, helpers, isa, &mut functions);
    functions
}

fn walk(
    proto: &FunctionProto,
    helpers: &JitHelpers,
    isa: &vn_jit::OwnedTargetIsa,
    out: &mut Vec<CompiledFunction>,
) {
    let name = proto.name.as_deref().unwrap_or("<module>").to_owned();
    let lines = (0..proto.chunk.code.len())
        .map(|offset| proto.chunk.lines.get_line(offset))
        .collect();
    if let Some(reason) = gate_reason(proto) {
        out.push(CompiledFunction {
            name,
            tier: Tier::Gate(reason),
            inspection: None,
            lines,
        });
    } else {
        let constants = constants_for_inspect(proto);
        let inspection = inspect(proto, &constants, helpers, isa);
        let tier = match &inspection.route {
            Ok(()) => Tier::Clif,
            Err(reason) => Tier::Bail(reason.clone()),
        };
        out.push(CompiledFunction {
            name,
            tier,
            inspection: Some(inspection),
            lines,
        });
    }
    for entry in &proto.chunk.constants {
        if let PoolEntry::Function(function) = entry {
            walk(function, helpers, isa, out);
        }
    }
}

fn header(title: &str, functions: &[CompiledFunction]) -> String {
    let native = functions
        .iter()
        .filter(|function| function.tier == Tier::Clif)
        .count();
    format!(
        "; {title}\n; {} function(s): {native} native, {} interpreted\n",
        functions.len(),
        functions.len() - native
    )
}

pub fn render_machine_code(functions: &[CompiledFunction]) -> String {
    let mut out = header(
        &format!("Varn machine code made by Cranelift for {}", host()),
        functions,
    );
    for function in functions {
        out.push_str(&format!(
            "\nfn {} [{}]\n",
            function.name,
            function.describe()
        ));
        let Some(inspection) = &function.inspection else {
            continue;
        };
        if let Some(code) = &inspection.code {
            render_code(function, inspection, code, &mut out);
        }
    }
    out
}

fn render_code(
    function: &CompiledFunction,
    inspection: &ClifInspection,
    code: &CodeBytes,
    out: &mut String,
) {
    for reason in &inspection.fa_reasons {
        out.push_str(&format!("  ; frame aware: {reason}\n"));
    }
    let length = code.bytes.len();
    let raw_end = (code.raw_off + code.raw_len).min(length);
    let entry = code.entry_off.min(length);
    out.push_str("  ; function body\n");
    push_instructions(
        function,
        &code.locations,
        &code.bytes[code.raw_off.min(length)..raw_end],
        code.raw_off as u64,
        out,
    );
    out.push_str("  ; entry wrapper, called by the VM\n");
    push_instructions(function, &[], &code.bytes[entry..], entry as u64, out);
}

fn push_instructions(
    function: &CompiledFunction,
    locations: &[(u32, u32)],
    bytes: &[u8],
    base: u64,
    out: &mut String,
) {
    for (offset, text) in disassemble(bytes, base) {
        let line = location_at(locations, offset).and_then(|ip| function.source_line(ip));
        let line = line.map_or_else(|| "-".to_owned(), |line| line.to_string());
        out.push_str(&format!("    {offset:04x}  {line:>4}  {text}\n"));
    }
}

fn location_at(locations: &[(u32, u32)], offset: u64) -> Option<u32> {
    locations
        .iter()
        .take_while(|(start, _)| u64::from(*start) <= offset)
        .last()
        .map(|(_, ip)| *ip)
}

pub fn render_clif(functions: &[CompiledFunction]) -> String {
    let mut out = header(
        "Varn Cranelift IR; `@NNNN` is the bytecode offset an instruction was made from",
        functions,
    );
    for function in functions {
        out.push_str(&format!(
            "\nfn {} [{}]\n",
            function.name,
            function.describe()
        ));
        let Some(ir) = function
            .inspection
            .as_ref()
            .and_then(|inspection| inspection.clif_ir.as_ref())
        else {
            continue;
        };
        for text in ir.lines() {
            out.push_str(&annotated(function, text));
            out.push('\n');
        }
    }
    out
}

fn annotated(function: &CompiledFunction, text: &str) -> String {
    let trimmed = text.trim_start();
    let line = trimmed
        .strip_prefix('@')
        .and_then(|rest| rest.get(..4))
        .and_then(|digits| u32::from_str_radix(digits, 16).ok())
        .and_then(|ip| function.source_line(ip));
    match line {
        Some(line) => format!("  {text}  ; line {line}"),
        None => format!("  {text}"),
    }
}

fn host() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86-64, Intel syntax"
    } else {
        "this host, as raw bytes"
    }
}

#[cfg(target_arch = "x86_64")]
fn disassemble(bytes: &[u8], base: u64) -> Vec<(u64, String)> {
    let mut decoder = Decoder::with_ip(64, bytes, base, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    let options = formatter.options_mut();
    options.set_hex_prefix("0x");
    options.set_hex_suffix("");
    options.set_uppercase_hex(false);
    options.set_branch_leading_zeros(false);
    options.set_small_hex_numbers_in_decimal(true);
    let mut instruction = Instruction::default();
    let mut lines = Vec::new();
    while decoder.can_decode() {
        decoder.decode_out(&mut instruction);
        let mut text = String::new();
        formatter.format(&instruction, &mut text);
        lines.push((instruction.ip(), text));
    }
    lines
}

#[cfg(not(target_arch = "x86_64"))]
fn disassemble(bytes: &[u8], base: u64) -> Vec<(u64, String)> {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(index, chunk)| {
            let hex: Vec<String> = chunk.iter().map(|byte| format!("{byte:02x}")).collect();
            (base + (index * 16) as u64, hex.join(" "))
        })
        .collect()
}
