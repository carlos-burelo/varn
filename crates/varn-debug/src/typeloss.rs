//! `vn debug -p typeloss` — where a statically typed program stops being one.
//!
//! Varn knows the type of nearly everything it compiles, and every generic
//! opcode is a place that knowledge did not survive to codegen. A type has to
//! get through six stages, and reading any one of them alone gives the wrong
//! answer: a hover can say `dynamic` for a loop whose bytecode is already
//! `AddInt`, and a `for…of` can look perfectly typed at the checker and lose
//! everything at HIR.
//!
//! This walks the two stages that can be attributed exactly — what the checker
//! published, and what the emitter produced — and reports only where they
//! disagree with what the language already knew.

use std::fmt::Write as _;

use varn_core::OpCode;
use varn_types::{FunctionProto, PoolEntry};

use crate::flags::DebugFlags;

const BOLD: &str = "\x1b[1m";
const BLUE: &str = "\x1b[34m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const R: &str = "\x1b[0m";

/// A typed opcode and the generic one it replaces. The pair is the unit of
/// measurement: `Add` alone says nothing, `Add` next to `AddInt` says the
/// emitter had a choice and took the dynamic one.
const PAIRS: &[(OpCode, OpCode)] = &[
    (OpCode::AddInt, OpCode::Add),
    (OpCode::SubInt, OpCode::Sub),
    (OpCode::MulInt, OpCode::Mul),
    (OpCode::EqInt, OpCode::Eq),
    (OpCode::LtInt, OpCode::Lt),
    (OpCode::GtInt, OpCode::Gt),
    (OpCode::GetFixedField, OpCode::GetProperty),
    (OpCode::SetFixedField, OpCode::SetProperty),
    (OpCode::ArrayGetIndex, OpCode::GetIndex),
    (OpCode::ArraySetIndex, OpCode::SetIndex),
    (OpCode::InvokeVirtual, OpCode::CallMethod),
];

#[derive(Default)]
struct Counts {
    typed: usize,
    generic: usize,
    /// Generic opcodes by name, worst first, for the detail column.
    by_op: Vec<(String, usize)>,
    /// The members that stayed name-keyed, read off the instruction's own
    /// constant rather than guessed from an annotation — the emitted opcode is
    /// the only thing that actually decides whether an access is dynamic.
    members: Vec<String>,
}

/// `-p typeloss`.
pub fn debug_typeloss(proto: &FunctionProto, flags: &DebugFlags) {
    eprintln!(
        "\n{BOLD}{BLUE}TYPELOSS{R}{DIM} ─────────────────────────────── {}{R}",
        proto.name.as_deref().unwrap_or("<top-level>")
    );

    let mut rows: Vec<(String, Counts)> = Vec::new();
    collect(proto, flags, &mut rows);
    rows.retain(|(_, c)| c.generic > 0);
    rows.sort_by(|a, b| b.1.generic.cmp(&a.1.generic));

    if rows.is_empty() {
        eprintln!("  {GREEN}cada opcode tipado que el emisor pudo elegir, lo eligió{R}");
        eprintln!("{DIM}── end: TYPELOSS ──{R}");
        return;
    }

    eprintln!(
        "\n  {DIM}{:<28} {:>8} {:>8}  {}{R}",
        "función", "genérico", "tipado", "qué quedó genérico"
    );
    for (name, c) in &rows {
        let mut detail = c
            .by_op
            .iter()
            .map(|(op, n)| {
                if *n == 1 {
                    op.clone()
                } else {
                    format!("{op}×{n}")
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        if !c.members.is_empty() {
            let _ = write!(detail, "  ·  {}", c.members.join(" "));
        }
        eprintln!(
            "  {:<28} {YELLOW}{:>8}{R} {:>8}  {DIM}{detail}{R}",
            truncate(name, 28),
            c.generic,
            c.typed
        );
    }
    eprintln!(
        "\n  {DIM}El siguiente paso es `-p check:types` sobre uno de estos accesos: un\n  \
         `cg=` sin `fixed_field` es la puerta de anotación, y sin `cg=` es inferencia.{R}"
    );
    eprintln!("{DIM}── end: TYPELOSS ──{R}");
}

fn collect(proto: &FunctionProto, flags: &DebugFlags, out: &mut Vec<(String, Counts)>) {
    let name = proto.name.as_deref().unwrap_or("<module>");
    if flags
        .fn_filter
        .as_ref()
        .is_none_or(|needle| name.contains(needle.as_str()))
    {
        out.push((name.to_owned(), count_one(proto)));
    }
    for entry in &proto.chunk.constants {
        if let PoolEntry::Function(f) = entry {
            collect(f, flags, out);
        }
    }
}

fn count_one(proto: &FunctionProto) -> Counts {
    let mut c = Counts::default();
    let mut generic: Vec<(String, usize)> = Vec::new();
    let code = &proto.chunk.code;
    let pool = &proto.chunk.constants;
    let mut ip = 0usize;
    while ip < code.len() {
        let Some(info) = varn_types::bytecode::decode(code, ip, pool) else {
            break;
        };
        if let Some(op) = OpCode::from_u8(code[ip] as u8) {
            if matches!(op, OpCode::GetProperty | OpCode::SetProperty) {
                // Both spell the member name in the constant two words along.
                if let Some(name) = code
                    .get(ip + 2)
                    .and_then(|idx| pool.get(*idx as usize))
                    .and_then(|entry| match entry {
                        PoolEntry::Literal(varn_types::Literal::Str(s)) => Some(s.to_string()),
                        _ => None,
                    })
                {
                    if !c.members.contains(&name) {
                        c.members.push(name);
                    }
                }
            }
            for (typed, gen) in PAIRS {
                if op == *typed {
                    c.typed += 1;
                } else if op == *gen {
                    c.generic += 1;
                    let label = format!("{op:?}");
                    match generic.iter_mut().find(|(n, _)| *n == label) {
                        Some((_, n)) => *n += 1,
                        None => generic.push((label, 1)),
                    }
                }
            }
        }
        ip += info.len;
    }
    generic.sort_by(|a, b| b.1.cmp(&a.1));
    c.by_op = generic;
    c
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    let mut out = String::with_capacity(max);
    let _ = write!(out, "{cut}…");
    out
}
