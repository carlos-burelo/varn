//! Bytecode listings, read off the layout table.
//!
//! One disassembler for every host: `vn debug` colours [`instructions`], the
//! editor shows [`render`]. Each instruction's length and operands come from
//! [`super::layout`], so a listing walks the stream exactly as the
//! interpreter does and names each operand for what it is.

use std::fmt::Write;

use varn_core::OpCode;

use super::{layout, Access, At, Byte, ImmKind, Layout, Operand};
use crate::chunk::{Chunk, Literal, PoolEntry};
use crate::FunctionProto;

/// One line of a listing.
#[derive(Clone, Debug)]
pub struct Instr {
    pub offset: usize,
    /// The source line, 0 when unknown.
    pub line: u32,
    /// `None` for a word that is not an opcode; the listing resumes at the
    /// next word.
    pub op: Option<OpCode>,
    pub len: usize,
    /// The operands: `r3 = r1 + r2`, `r4 = #2`, `r5 → 0031`.
    pub text: String,
    /// The constant-pool entries the instruction names, in operand order,
    /// for a listing to annotate ([`constant_text`]).
    pub constants: Vec<usize>,
}

/// The instructions of `chunk`, in order.
pub fn instructions(chunk: &Chunk) -> impl Iterator<Item = Instr> + '_ {
    let code = &chunk.code;
    let mut offset = 0;
    std::iter::from_fn(move || {
        if offset >= code.len() {
            return None;
        }
        let line = chunk.lines.get_line(offset);
        let instr = match layout(code, offset, &chunk.constants) {
            Some(l) => Instr {
                offset,
                line,
                op: Some(l.op),
                len: l.len,
                text: operands_text(&l, code, offset),
                constants: l
                    .operands
                    .iter()
                    .filter_map(|o| match *o {
                        Operand::Const { word, .. } => {
                            Some(At::Word(word).read(code, offset) as usize)
                        }
                        _ => None,
                    })
                    .collect(),
            },
            None => Instr {
                offset,
                line,
                op: None,
                len: 1,
                text: format!("raw {:#06x}", code[offset]),
                constants: Vec::new(),
            },
        };
        offset += instr.len;
        Some(instr)
    })
}

/// A listing of `proto` and every function nested in its constants.
pub fn render(proto: &FunctionProto) -> String {
    let mut out = String::new();
    render_into(proto, &mut out);
    out
}

fn render_into(proto: &FunctionProto, out: &mut String) {
    let name = proto.name.as_deref().unwrap_or("<anonymous>");
    let flags: Vec<&str> = [
        proto.is_async.then_some("async"),
        proto.is_generator.then_some("generator"),
        proto.has_this.then_some("this"),
        proto.has_rest.then_some("rest"),
    ]
    .into_iter()
    .flatten()
    .collect();
    let _ = write!(
        out,
        "fn {name} (arity {}, registers {}, upvalues {})",
        proto.arity, proto.register_count, proto.upvalue_count
    );
    if !flags.is_empty() {
        let _ = write!(out, " [{}]", flags.join(", "));
    }
    out.push('\n');

    let constants = &proto.chunk.constants;
    if !constants.is_empty() {
        out.push_str("  constants\n");
        for (i, c) in constants.iter().enumerate() {
            let _ = writeln!(out, "    #{i:<3} {}", constant_text(c));
        }
    }
    let _ = writeln!(out, "  code ({} words)", proto.chunk.code.len());
    for instr in instructions(&proto.chunk) {
        let op = instr
            .op
            .map_or_else(|| "???".to_owned(), |op| format!("{op:?}"));
        let _ = write!(
            out,
            "    {:04}  {:>4}  {op:<20} {}",
            instr.offset, instr.line, instr.text
        );
        let notes: Vec<String> = instr
            .constants
            .iter()
            .filter_map(|&i| constants.get(i).map(constant_text))
            .collect();
        if !notes.is_empty() {
            let _ = write!(out, "  ; {}", notes.join(", "));
        }
        out.push('\n');
    }
    out.push('\n');

    for c in constants {
        if let PoolEntry::Function(nested) = c {
            render_into(nested, out);
        }
    }
}

/// A constant as a listing annotates it: `"text"`, `42`, `fn name`,
/// `shape {a, b}`.
pub fn constant_text(entry: &PoolEntry) -> String {
    match entry {
        PoolEntry::Literal(lit) => match lit {
            Literal::Null => "null".to_owned(),
            Literal::Bool(b) => b.to_string(),
            Literal::Int(n) => n.to_string(),
            Literal::Float(f) => format!("{f:?}"),
            Literal::Str(s) => format!("{s:?}"),
            Literal::BigInt(n) => format!("{n}n"),
            Literal::Decimal(d) => format!("{d}d"),
            Literal::Char(c) => format!("{c:?}"),
            Literal::Symbol(s) => format!("Symbol.{}", s.name()),
        },
        PoolEntry::Function(f) => format!("fn {}", f.name.as_deref().unwrap_or("<anonymous>")),
        PoolEntry::Shape(keys) => format!("shape {{{}}}", keys.join(", ")),
    }
}

/// The operands of the instruction at `offset`: what it writes, `=`, then
/// what it reads and names.
pub fn operands_text(layout: &Layout, code: &[u16], offset: usize) -> String {
    let reg = |at: Byte| format!("r{}", at.read(code, offset));
    let mut dest = None;
    let mut args: Vec<String> = Vec::new();
    for operand in &layout.operands {
        match *operand {
            Operand::Reg { at, access } => {
                if access != Access::Read {
                    dest = Some(reg(at));
                }
                if access != Access::Write {
                    args.push(reg(at));
                }
            }
            Operand::Fixed { reg, access } => {
                if access != Access::Read {
                    dest = Some(format!("r{reg}"));
                }
                if access != Access::Write {
                    args.push(format!("r{reg}"));
                }
            }
            Operand::Run { start, count, .. } => {
                let first = start.read(code, offset) as usize;
                args.push(match count {
                    0 => "()".to_owned(),
                    1 => format!("(r{first})"),
                    n => format!("(r{first}..r{})", first + n - 1),
                });
            }
            Operand::Const { word, .. } => {
                args.push(format!("#{}", At::Word(word).read(code, offset)));
            }
            Operand::Imm { at, kind } => args.extend(imm_text(at, kind, code, offset)),
            Operand::Jump { .. } => {
                if let Some(target) = layout.jump_target(code, offset) {
                    args.push(format!("→ {target:04}"));
                }
            }
        }
    }

    let body = match (implied(layout.op), infix(layout.op), prefix(layout.op)) {
        (Some(value), _, _) => value.to_owned(),
        (_, Some(sym), _) if args.len() == 2 => format!("{} {sym} {}", args[0], args[1]),
        (_, _, Some(sym)) if args.len() == 1 => format!("{sym}{}", args[0]),
        _ => args.join(", "),
    };
    match dest {
        Some(d) if body.is_empty() => d,
        Some(d) => format!("{d} = {body}"),
        None => body,
    }
}

fn imm_text(at: At, kind: ImmKind, code: &[u16], offset: usize) -> Option<String> {
    let v = at.read(code, offset);
    Some(match kind {
        ImmKind::Int => match at {
            At::Byte(_) => (v as u8 as i8).to_string(),
            At::Word(_) => (v as i16).to_string(),
        },
        // The list it counts is spelled out.
        ImmKind::Count => return None,
        ImmKind::Upvalue => format!("uv{v}"),
        ImmKind::CallSite => format!("ic{v}"),
        ImmKind::GlobalSlot => format!("global@{v}"),
        ImmKind::NativeGlobalSlot => format!("native@{v}"),
        ImmKind::ModuleSlot => format!("slot{v}"),
        ImmKind::FieldSlot => format!("field{v}"),
        ImmKind::FieldOffset => format!("+{v}"),
        ImmKind::Tag => match varn_core::RuntimeKind::from_u8(v as u8) {
            Some(kind) => format!(":{}", kind.name()),
            None => ":dynamic".to_owned(),
        },
        ImmKind::Conv => match varn_core::NumConv::from_u8(v as u8) {
            Some(conv) => format!("{conv:?}"),
            None => format!("conv{v}"),
        },
        ImmKind::Intrinsic => format!("math{v:#04x}"),
        ImmKind::Flag => format!("flag{v}"),
    })
}

/// The value an operand-less load puts in its register.
fn implied(op: OpCode) -> Option<&'static str> {
    Some(match op {
        OpCode::LoadNull => "null",
        OpCode::LoadTrue => "true",
        OpCode::LoadFalse => "false",
        OpCode::LoadIntZero => "0",
        OpCode::LoadIntOne => "1",
        OpCode::LoadIntMinusOne => "-1",
        _ => return None,
    })
}

/// The operator a two-operand instruction applies.
fn infix(op: OpCode) -> Option<&'static str> {
    use OpCode as O;
    Some(match op {
        O::Add | O::AddInt | O::AddFloat | O::AddImm | O::StrConcat => "+",
        O::Sub | O::SubInt | O::SubFloat | O::SubImm => "-",
        O::Mul | O::MulInt | O::MulFloat => "*",
        O::Div | O::DivInt | O::DivFloat => "/",
        O::Mod | O::ModInt | O::ModFloat => "%",
        O::Pow | O::PowInt | O::PowFloat => "**",
        O::BitAnd => "&",
        O::BitOr => "|",
        O::BitXor => "^",
        O::Shl => "<<",
        O::Shr => ">>",
        O::Ushr => ">>>",
        O::Eq | O::EqInt | O::EqFloat => "==",
        O::Neq | O::NeqInt | O::NeqFloat => "!=",
        O::Lt | O::LtInt | O::LtFloat => "<",
        O::Lte | O::LteInt | O::LteFloat => "<=",
        O::Gt | O::GtInt | O::GtFloat => ">",
        O::Gte | O::GteInt | O::GteFloat => ">=",
        O::In => "in",
        O::Instanceof => "instanceof",
        _ => return None,
    })
}

/// The operator a one-operand instruction applies.
fn prefix(op: OpCode) -> Option<&'static str> {
    Some(match op {
        OpCode::Negate => "-",
        OpCode::Not => "!",
        _ => return None,
    })
}
