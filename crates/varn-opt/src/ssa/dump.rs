//! Textual dump of an [`SsaFunc`], for `vn debug -p ssa` and tests.
//!
//! Deterministic, compact, one instruction per line. Block parameters render as
//! `bN(v0: int, v1: float):`; branch/jump terminators show their block-argument
//! lists so the SSA merge operands are visible.

use std::fmt::Write;

use crate::hir::{HirBinOp, HirType, HirUnOp};

use super::ir::{Block, InstKind, SsaFunc, Terminator, Value};

pub fn dump(func: &SsaFunc) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "fn {}:", func.name);
    for (i, block) in func.blocks.iter().enumerate() {
        dump_block(&mut out, func, i as u32, block);
    }
    out
}

fn dump_block(out: &mut String, func: &SsaFunc, id: u32, block: &Block) {
    let params = block
        .params
        .iter()
        .map(|v| format!("{}: {}", val(*v), ty(func.value_ty(*v))))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(out, "  b{id}({params}):");
    for inst in &block.insts {
        let lhs = match inst.dest {
            Some(v) => format!("{} = ", val(v)),
            None => String::new(),
        };
        let _ = writeln!(out, "    {lhs}{}", inst_kind(&inst.kind));
    }
    let _ = writeln!(out, "    {}", terminator(&block.term));
}

fn inst_kind(kind: &InstKind) -> String {
    match kind {
        InstKind::ConstInt(n) => format!("int {n}"),
        InstKind::ConstFloat(n) => format!("float {n}"),
        InstKind::ConstBool(b) => format!("bool {b}"),
        InstKind::ConstStr(s) => format!("str {s:?}"),
        InstKind::ConstChar(c) => format!("char {c:?}"),
        InstKind::ConstDecimal(d) => format!("decimal {d}"),
        InstKind::ConstBigInt(n) => format!("bigint {n}"),
        InstKind::ConstNull => "null".to_owned(),
        InstKind::Binary { op, lhs, rhs, ty: t } => {
            format!("{}.{} {}, {}", binop(*op), ty(*t), val(*lhs), val(*rhs))
        }
        InstKind::Unary { op, operand, ty: t } => {
            format!("{}.{} {}", unop(*op), ty(*t), val(*operand))
        }
        InstKind::LoadGlobal(name) => format!("global {name}"),
        InstKind::Call { callee, args } => format!("call {}{}", val(*callee), args_list(args)),
        InstKind::SelfCall { args } => format!("callself{}", args_list(args)),
        InstKind::GetProperty { object, name } => format!("getprop {}.{name}", val(*object)),
        InstKind::GetIndex { object, index } => {
            format!("getindex {}[{}]", val(*object), val(*index))
        }
        InstKind::SetProperty { object, name, value } => {
            format!("setprop {}.{name} = {}", val(*object), val(*value))
        }
        InstKind::SetIndex { object, index, value } => {
            format!("setindex {}[{}] = {}", val(*object), val(*index), val(*value))
        }
        InstKind::MethodCall { recv, name, args } => {
            format!("callmethod {}.{name}{}", val(*recv), args_list(args))
        }
        InstKind::IsNull { operand } => format!("isnull {}", val(*operand)),
        InstKind::BuildArray { elements } => format!("array{}", args_list(elements)),
        InstKind::BuildObject { pairs } => {
            let inner = pairs
                .iter()
                .map(|(k, v)| format!("{k}: {}", val(*v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("object {{{inner}}}")
        }
        InstKind::ToString { operand } => format!("tostring {}", val(*operand)),
        InstKind::BuildStr { parts } => format!("buildstr{}", args_list(parts)),
        InstKind::MakeClosure { func } => format!("closure {}", func.name),
        InstKind::IntrinsicCall { object, args, wire_byte } => {
            format!("intrinsic#{wire_byte} {}{}", val(*object), args_list(args))
        }
        InstKind::AssertNotNull { operand } => format!("assertnotnull {}", val(*operand)),
        InstKind::GetPropertyMaybe { object, name } => {
            format!("getpropmaybe {}.{name}", val(*object))
        }
        InstKind::ModuleSlot { object, slot } => format!("moduleslot {}[{slot}]", val(*object)),
    }
}

fn terminator(term: &Terminator) -> String {
    match term {
        Terminator::Return(Some(v)) => format!("return {}", val(*v)),
        Terminator::Return(None) => "return".to_owned(),
        Terminator::Jump { target, args } => format!("jump b{}{}", target.0, args_list(args)),
        Terminator::Branch {
            cond,
            then_blk,
            then_args,
            else_blk,
            else_args,
        } => format!(
            "branch {}, b{}{}, b{}{}",
            val(*cond),
            then_blk.0,
            args_list(then_args),
            else_blk.0,
            args_list(else_args),
        ),
        Terminator::Unreachable => "unreachable".to_owned(),
    }
}

fn args_list(args: &[Value]) -> String {
    if args.is_empty() {
        String::new()
    } else {
        let inner = args.iter().map(|v| val(*v)).collect::<Vec<_>>().join(", ");
        format!("({inner})")
    }
}

fn val(v: Value) -> String {
    format!("v{}", v.0)
}

fn ty(t: HirType) -> &'static str {
    match t {
        HirType::Int => "int",
        HirType::Float => "float",
        HirType::Bool => "bool",
        HirType::Str => "str",
        HirType::Ref => "ref",
        HirType::Dynamic => "dyn",
    }
}

fn binop(op: HirBinOp) -> &'static str {
    match op {
        HirBinOp::Add => "add",
        HirBinOp::Sub => "sub",
        HirBinOp::Mul => "mul",
        HirBinOp::Div => "div",
        HirBinOp::Mod => "mod",
        HirBinOp::Pow => "pow",
        HirBinOp::Eq => "eq",
        HirBinOp::Ne => "ne",
        HirBinOp::Lt => "lt",
        HirBinOp::Le => "le",
        HirBinOp::Gt => "gt",
        HirBinOp::Ge => "ge",
        HirBinOp::And => "and",
        HirBinOp::Or => "or",
        HirBinOp::BitAnd => "band",
        HirBinOp::BitOr => "bor",
        HirBinOp::BitXor => "bxor",
        HirBinOp::Shl => "shl",
        HirBinOp::Shr => "shr",
        HirBinOp::Ushr => "ushr",
        HirBinOp::Instanceof => "instanceof",
        HirBinOp::In => "in",
    }
}

fn unop(op: HirUnOp) -> &'static str {
    match op {
        HirUnOp::Neg => "neg",
        HirUnOp::Not => "not",
        HirUnOp::BitNot => "bnot",
        HirUnOp::Typeof => "typeof",
    }
}
