//! Scalar replacement of class instances that never escape.
//!
//! The sibling of `fixed_fields`, one level harder. That pass forwards reads
//! of an object literal whose contents are already in SSA form; here the
//! contents are produced by a *constructor call*, so the values have to come
//! from a cross-function summary (`hir::ctor_summary`) instead of from the
//! instruction itself.
//!
//! ```text
//!   v6 = global M::Box              v6 = global M::Box   (DCE removes)
//!   v7 = call v6(v3)          -->   (deleted)
//!   v9 = getfixed v7[0]             v9 forwarded to v3
//! ```
//!
//! Once every read is forwarded the call has no uses left, and THIS pass
//! deletes it — DCE cannot, because a `Call` is an effect as far as it knows
//! and it has no way to tell that this particular callee only fills in a
//! fresh object nobody can reach.
//!
//! Disqualification mirrors `fixed_fields` and is conservative: any use of
//! the instance other than a `GetFixedField` on it — call argument, store,
//! return, branch argument, `SetFixedField`, `GetProperty` — keeps the
//! allocation. Object identity is observable here (`===` is the `Rc`
//! address), and every one of those uses can leak it.
//!
//! The summary's own soundness argument — why a class global may be resolved
//! at compile time at all, when `Box = Other` is legal — is in
//! `hir::ctor_summary`.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::hir::ctor_summary::{CtorSummaries, SlotInit};
use crate::ssa::ir::{InstKind, SsaFunc, Terminator, Value};
use crate::ssa::verify::inst_uses;

pub fn run(func: &mut SsaFunc, summaries: &CtorSummaries) -> bool {
    if summaries.is_empty() {
        return false;
    }

    // `global M::C` defs, so a call's callee can be resolved to a class.
    let mut globals: FxHashMap<u32, &str> = FxHashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let (Some(d), InstKind::LoadGlobal(name)) = (inst.dest, &inst.kind) {
                globals.insert(d.0, name);
            }
        }
    }
    if globals.is_empty() {
        return false;
    }

    // Construction sites: instance value → (constructor args, slot summary).
    let mut sites: FxHashMap<u32, (Vec<Value>, &Vec<SlotInit>)> = FxHashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            let (Some(dest), InstKind::Call { callee, args }) = (inst.dest, &inst.kind) else {
                continue;
            };
            let Some(slots) = globals.get(&callee.0).and_then(|n| summaries.get(*n)) else {
                continue;
            };
            // The summary indexes PARAMETERS; a call passing fewer arguments
            // than the constructor declares is not the shape it describes.
            if slots
                .iter()
                .any(|s| matches!(s, SlotInit::Param(p) if *p as usize >= args.len()))
            {
                continue;
            }
            sites.insert(dest.0, (args.clone(), slots));
        }
    }
    if sites.is_empty() {
        return false;
    }

    // Anything that could observe the instance itself disqualifies it.
    for block in &func.blocks {
        for inst in &block.insts {
            if let InstKind::GetFixedField { object, .. } = &inst.kind {
                if sites.contains_key(&object.0) {
                    continue;
                }
            }
            for u in inst_uses(&inst.kind) {
                sites.remove(&u.0);
            }
        }
        match &block.term {
            Terminator::Return(Some(v)) | Terminator::Throw(v) => {
                sites.remove(&v.0);
            }
            Terminator::Branch {
                cond,
                then_args,
                else_args,
                ..
            } => {
                sites.remove(&cond.0);
                for a in then_args.iter().chain(else_args) {
                    sites.remove(&a.0);
                }
            }
            Terminator::Jump { args, .. } => {
                for a in args {
                    sites.remove(&a.0);
                }
            }
            _ => {}
        }
    }
    if sites.is_empty() {
        return false;
    }

    // Collect the forwards, and note any instance with a read this pass
    // cannot answer: a slot the constructor left null (whose SSA type would
    // then disagree with the value) or one outside the summary. Such an
    // instance keeps its allocation — the reads that DID resolve are still
    // forwarded, which is correct, just not a win on its own.
    let mut forwards: Vec<(Value, Value)> = Vec::new();
    let mut unresolved: FxHashSet<u32> = FxHashSet::default();
    for block in &func.blocks {
        for inst in &block.insts {
            let (Some(dest), InstKind::GetFixedField { object, slot }) = (inst.dest, &inst.kind)
            else {
                continue;
            };
            let Some((args, slots)) = sites.get(&object.0) else {
                continue;
            };
            match slots.get(*slot as usize) {
                Some(SlotInit::Param(p)) => forwards.push((dest, args[*p as usize])),
                _ => {
                    unresolved.insert(object.0);
                }
            }
        }
    }
    if forwards.is_empty() {
        return false;
    }

    for (dest, value) in forwards {
        func.replace_all_uses(dest, value);
    }

    // Delete the constructions whose every read was answered. Sound by the
    // summary: the callee writes nothing but fields of an object that, per
    // the disqualification above, nothing else in this function can reach.
    let dead: FxHashSet<u32> = sites
        .keys()
        .copied()
        .filter(|v| !unresolved.contains(v))
        .collect();
    for block in &mut func.blocks {
        block.insts.retain(|inst| match (inst.dest, &inst.kind) {
            (Some(d), InstKind::Call { .. }) => !dead.contains(&d.0),
            _ => true,
        });
    }
    true
}
