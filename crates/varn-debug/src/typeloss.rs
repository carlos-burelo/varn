use std::fmt::Write as _;

use varn_core::OpCode;
use varn_types::bytecode::{ConstKind, Operand};
use varn_types::{FunctionProto, PoolEntry};

use crate::render::truncate;

use crate::flags::DebugFlags;

use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_core::term::terminal::{Align, Section};

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

    by_op: Vec<(String, usize)>,

    members: Vec<String>,
}

pub fn debug_typeloss(proto: &FunctionProto, flags: &DebugFlags, module: Option<&str>) {
    let mut rows: Vec<(String, Counts)> = Vec::new();
    collect(proto, flags, &mut rows);
    rows.retain(|(_, c)| c.generic > 0);
    rows.sort_by_key(|row| std::cmp::Reverse(row.1.generic));

    if rows.is_empty() {
        if module.is_some() {
            return;
        }
        Section::new("typeloss")
            .subtitle(proto.name.as_deref().unwrap_or("<top-level>"))
            .color(|c| c.blue())
            .print();
        terminal::info("cada opcode tipado que el emisor pudo elegir, lo eligió");
        Section::new("typeloss").close();
        return;
    }

    if let Some(m) = module {
        terminal::tagged("module", m);
    }
    Section::new("typeloss")
        .subtitle(proto.name.as_deref().unwrap_or("<top-level>"))
        .color(|c| c.blue())
        .print();

    let mut table = terminal::Table::new(["función", "genérico", "tipado", "qué quedó genérico"])
        .align([Align::Left, Align::Right, Align::Right, Align::Left]);
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
        table.row([
            truncate(name, 28).to_string(),
            c.generic.to_string(),
            c.typed.to_string(),
            detail,
        ]);
    }
    table.print();
    terminal::log(format!(
        "  {}",
        chalk(
            "El siguiente paso es `-p check:types` sobre uno de estos accesos: un \
             `cg=` sin `fixed_field` es la puerta de anotación, y sin `cg=` es inferencia."
        )
        .dim()
    ));
    Section::new("typeloss").close();
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
        let Some(layout) = varn_types::bytecode::layout(code, ip, pool) else {
            break;
        };
        let op = layout.op;
        if matches!(op, OpCode::GetProperty | OpCode::SetProperty) {
            let name = layout.operands.iter().find_map(|o| match *o {
                Operand::Const {
                    word,
                    kind: ConstKind::Name,
                } => match code.get(ip + word).and_then(|&i| pool.get(i as usize)) {
                    Some(PoolEntry::Literal(varn_types::Literal::Str(s))) => Some(s.to_string()),
                    None | Some(_) => None,
                },
                Operand::Reg { .. }
                | Operand::Run { .. }
                | Operand::Fixed { .. }
                | Operand::Const { .. }
                | Operand::Imm { .. }
                | Operand::Jump { .. } => None,
            });
            if let Some(name) = name {
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
        ip += layout.len;
    }
    generic.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    c.by_op = generic;
    c
}
