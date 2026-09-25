//! The compiled control-flow graph: which blocks compiled code reaches, in
//! what order, from where, and which loops can reach a safepoint.

use varn_types::ssa::{SsaProto, SsaTerm};

use super::store::clif_ty;
use super::views;

/// Each block's predecessors through terminators.
pub(super) fn predecessors(ssa: &SsaProto) -> Vec<Vec<usize>> {
    let mut preds = vec![Vec::new(); ssa.blocks.len()];
    for (b, blk) in ssa.blocks.iter().enumerate() {
        let succs: &[u32] = match &blk.term {
            SsaTerm::Jump { target, .. } => std::slice::from_ref(target),
            SsaTerm::Branch {
                then_blk, else_blk, ..
            } => &[*then_blk, *else_blk],
            SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Unreachable => &[],
        };
        for s in succs {
            preds[*s as usize].push(b);
        }
    }
    preds
}

/// Whether the loop closed by the back edge `latch -> header` can reach a
/// safepoint: an instruction outside [`views::keeps_views`]' allowlist in any
/// block of its body (the blocks that reach `latch` without passing
/// `header`). A loop that cannot allocate has nothing to collect, so its back
/// edge needs no poll.
pub(super) fn loop_may_collect(
    ssa: &SsaProto,
    preds: &[Vec<usize>],
    header: usize,
    latch: usize,
) -> bool {
    let mut seen = vec![false; ssa.blocks.len()];
    seen[header] = true;
    let mut stack = vec![latch];
    let mut body = vec![header];
    while let Some(b) = stack.pop() {
        if std::mem::replace(&mut seen[b], true) {
            continue;
        }
        body.push(b);
        stack.extend(preds[b].iter().copied());
    }
    body.iter().any(|&b| {
        ssa.blocks[b]
            .insts
            .iter()
            .any(|i| !views::keeps_views(ssa, &i.op))
    })
}

/// The blocks compiled code can reach — through terminators from `entry`,
/// the function's entry block or an OSR entry's loop header — in reverse
/// postorder, so every value is defined before its uses. A block reachable
/// only through a `try` handler is a landing pad or the catch path behind it,
/// which runs interpreted (see [`super::exceptions`]).
pub(super) fn order(ssa: &SsaProto, entry: usize) -> Vec<usize> {
    let n = ssa.blocks.len();
    let mut visited = vec![false; n];
    let mut post = Vec::with_capacity(n);
    let mut stack: Vec<(usize, u8)> = vec![(entry, 0)];
    visited[entry] = true;
    while let Some(&(b, stage)) = stack.last() {
        stack.last_mut().unwrap().1 += 1;
        let succs: Vec<usize> = match &ssa.blocks[b].term {
            SsaTerm::Jump { target, .. } => vec![*target as usize],
            SsaTerm::Branch {
                then_blk, else_blk, ..
            } => vec![*else_blk as usize, *then_blk as usize],
            _ => Vec::new(),
        };
        match succs.get(stage as usize) {
            Some(&s) => {
                if !visited[s] {
                    visited[s] = true;
                    stack.push((s, 0));
                }
            }
            None => {
                post.push(b);
                stack.pop();
            }
        }
    }
    post.into_iter().rev().collect()
}

/// Every jump argument has its parameter's representation. The compiler
/// brings each value reaching a merge to the merge's type; a body where one
/// does not is declined here, by name, rather than handed to Cranelift — which
/// only checks this when its verifier runs, and it does not in release.
pub(super) fn check_block_args(ssa: &SsaProto) -> Result<(), String> {
    let check = |from: usize, target: u32, args: &[u32]| -> Result<(), String> {
        let params = &ssa.blocks[target as usize].params;
        if params.len() != args.len() {
            return Err(format!(
                "from_ssa: block {from} passes {} values to block {target}, which takes {}",
                args.len(),
                params.len()
            ));
        }
        for (&a, &p) in args.iter().zip(params) {
            let (ak, pk) = (ssa.value_ty(a), ssa.value_ty(p));
            if clif_ty(ak) != clif_ty(pk) {
                return Err(format!(
                    "from_ssa: block {from} passes {ak:?} value {a} to {pk:?} parameter {p} of block {target}"
                ));
            }
        }
        Ok(())
    };
    for (b, blk) in ssa.blocks.iter().enumerate() {
        match &blk.term {
            SsaTerm::Jump { target, args } => check(b, *target, args)?,
            SsaTerm::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => {
                check(b, *then_blk, then_args)?;
                check(b, *else_blk, else_args)?;
            }
            SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Unreachable => {}
        }
    }
    Ok(())
}
