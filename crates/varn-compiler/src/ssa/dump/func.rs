use super::super::ir::{Block, SsaFunc, Terminator};
use super::insts::inst_kind;
use super::operands::{args_list, ty, val};
use crate::hir::HirType;
use std::fmt::Write;

pub fn dump(func: &SsaFunc) -> String {
    let mut out = String::new();
    let mut flags = Vec::new();
    if func.is_async {
        flags.push("async");
    }
    if func.is_generator {
        flags.push("generator");
    }
    if flags.is_empty() {
        let _ = writeln!(out, "fn {}:", func.name);
    } else {
        let _ = writeln!(out, "fn {}: [{}]", func.name, flags.join(", "));
    }
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
            Some(v) => match func.value_ty(v) {
                HirType::Dynamic => format!("{} = ", val(v)),
                t @ HirType::Int
                | t @ HirType::Float
                | t @ HirType::Bool
                | t @ HirType::Str
                | t @ HirType::Ref
                | t @ HirType::Array(_)
                | t @ HirType::Map(..)
                | t @ HirType::Set(_)
                | t @ HirType::Class(_)
                | t @ HirType::Nullable(_) => format!("{}: {} = ", val(v), ty(t)),
            },
            None => String::new(),
        };
        let _ = writeln!(out, "    {lhs}{}", inst_kind(&inst.kind));
    }
    let _ = writeln!(out, "    {}", terminator(&block.term));
}

pub fn terminator(term: &Terminator) -> String {
    match term {
        Terminator::Return(Some(v)) => format!("return {}", val(*v)),
        Terminator::Return(None) => "return".to_owned(),
        Terminator::Throw(v) => format!("throw {}", val(*v)),
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
