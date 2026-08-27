//! Block terminators: the jumps, the branch, and the register copies that
//! realize phi nodes on the edge into a successor.

use super::super::ir::{BlockId, SsaFunc, Terminator, Value};
use crate::OptError;
use varn_core::OpCode;
use varn_types::chunk::Chunk;

type Result<T> = std::result::Result<T, OptError>;

pub(super) fn emit_call_args(chunk: &mut Chunk, reg: &[u8], call_base: u8, args: &[Value], line: u32) {
    chunk.emit_rr(OpCode::LoadNull, call_base, 0, line);
    for (i, a) in args.iter().enumerate() {
        chunk.emit_rr(
            OpCode::Move,
            call_base + 1 + i as u8,
            reg[a.0 as usize],
            line,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_terminator(
    chunk: &mut Chunk,
    ssa: &SsaFunc,
    reg: &[u8],
    cur_pos: usize,
    pos_of: &[usize],
    term: &Terminator,
    line: u32,
    null_reg: u8,
    scratch: u8,
    block_offset: &[usize],
    fixups: &mut Vec<(usize, BlockId)>,
) -> Result<()> {
    match term {
        Terminator::Return(Some(v)) => {
            chunk.emit1(OpCode::Return, Chunk::pack(0, reg[v.0 as usize]), line);
        }
        Terminator::Return(None) | Terminator::Unreachable => {
            chunk.write(Chunk::pack_op(OpCode::LoadNull, null_reg), line);
            chunk.emit1(OpCode::Return, Chunk::pack(0, null_reg), line);
        }
        Terminator::Throw(v) => {
            chunk.emit1(OpCode::Throw, Chunk::pack(reg[v.0 as usize], 0), line);
        }
        Terminator::Jump { target, args } => {
            emit_edge_copies(chunk, ssa, reg, *target, args, scratch, line);
            emit_goto(chunk, cur_pos, pos_of, *target, block_offset, fixups, line);
        }
        Terminator::Branch {
            cond,
            then_blk,
            then_args,
            else_blk,
            else_args,
        } => {
            if !then_args.is_empty() || !else_args.is_empty() {
                return Err(OptError::Unsupported(
                    "ssa-emit: branch edge carries args after split",
                ));
            }
            emit_branch(
                chunk,
                cur_pos,
                pos_of,
                reg[cond.0 as usize],
                *then_blk,
                *else_blk,
                block_offset,
                fixups,
                line,
            )?;
        }
    }
    Ok(())
}

fn emit_edge_copies(
    chunk: &mut Chunk,
    ssa: &SsaFunc,
    reg: &[u8],
    target: BlockId,
    args: &[Value],
    scratch: u8,
    line: u32,
) {
    let params = &ssa.blocks[target.0 as usize].params;
    let mut copies: Vec<(u8, u8)> = params
        .iter()
        .zip(args)
        .map(|(p, a)| (reg[p.0 as usize], reg[a.0 as usize]))
        .filter(|(d, s)| d != s)
        .collect();

    while !copies.is_empty() {
        if let Some(pos) = copies
            .iter()
            .position(|(d, _)| !copies.iter().any(|(_, s)| s == d))
        {
            let (d, s) = copies.remove(pos);
            chunk.emit_rr(OpCode::Move, d, s, line);
        } else {
            let s0 = copies[0].1;
            chunk.emit_rr(OpCode::Move, scratch, s0, line);
            for c in copies.iter_mut() {
                if c.1 == s0 {
                    c.1 = scratch;
                }
            }
        }
    }
}

fn emit_goto(
    chunk: &mut Chunk,
    cur_pos: usize,
    pos_of: &[usize],
    target: BlockId,
    block_offset: &[usize],
    fixups: &mut Vec<(usize, BlockId)>,
    line: u32,
) {
    let tp = pos_of[target.0 as usize];
    if tp == cur_pos + 1 {
        return;
    }
    if tp <= cur_pos {
        chunk.emit_loop(block_offset[target.0 as usize], line);
    } else {
        let pos = chunk.emit_jump(OpCode::Jump, line);
        fixups.push((pos, target));
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_branch(
    chunk: &mut Chunk,
    cur_pos: usize,
    pos_of: &[usize],
    cond: u8,
    then_blk: BlockId,
    else_blk: BlockId,
    block_offset: &[usize],
    fixups: &mut Vec<(usize, BlockId)>,
    line: u32,
) -> Result<()> {
    let tb = then_blk.0 as usize;
    let eb = else_blk.0 as usize;
    let then_fwd = pos_of[tb] > cur_pos;
    let else_fwd = pos_of[eb] > cur_pos;

    match (then_fwd, else_fwd) {
        (true, true) => {
            if pos_of[tb] == cur_pos + 1 {
                let pos = chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, line);
                fixups.push((pos, else_blk));
            } else if pos_of[eb] == cur_pos + 1 {
                let pos = chunk.emit_cond_jump(OpCode::JumpIfTrue, cond, line);
                fixups.push((pos, then_blk));
            } else {
                let p1 = chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, line);
                fixups.push((p1, else_blk));
                let p2 = chunk.emit_jump(OpCode::Jump, line);
                fixups.push((p2, then_blk));
            }
        }
        (true, false) => {
            let pos = chunk.emit_cond_jump(OpCode::JumpIfTrue, cond, line);
            fixups.push((pos, then_blk));
            chunk.emit_loop(block_offset[eb], line);
        }
        (false, true) => {
            let pos = chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, line);
            fixups.push((pos, else_blk));
            chunk.emit_loop(block_offset[tb], line);
        }
        (false, false) => {
            return Err(OptError::Unsupported(
                "ssa-emit: branch with both targets backward",
            ));
        }
    }
    Ok(())
}
