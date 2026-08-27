//! Redundant Guard and Null-Check Elimination Pass.
//!
//! Eliminates redundant `IsNull` instructions and simplifies branches by propagating
//! non-null and null facts along single-predecessor control-flow branches and from
//! non-null constructor/literal definitions.

use crate::hir::HirBinOp;
use crate::ssa::ir::{BlockId, InstKind, SsaFunc, Terminator, Value};
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NullFact {
    Null,
    NonNull,
}

pub fn run(func: &mut SsaFunc) -> bool {
    let mut changed = false;

    // 1. Identify inherently non-null SSA values (allocations, constants, `this`)
    let mut inherently_non_null: FxHashSet<Value> = FxHashSet::default();
    let mut null_literals: FxHashSet<Value> = FxHashSet::default();

    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(dest) = inst.dest {
                match &inst.kind {
                    InstKind::ConstNull => {
                        null_literals.insert(dest);
                    }
                    InstKind::ConstInt(_)
                    | InstKind::ConstFloat(_)
                    | InstKind::ConstBool(_)
                    | InstKind::ConstStr(_)
                    | InstKind::ConstChar(_)
                    | InstKind::ConstDecimal(_)
                    | InstKind::ConstBigInt(_)
                    | InstKind::BuildArray { .. }
                    | InstKind::BuildTuple { .. }
                    | InstKind::BuildObject { .. }
                    | InstKind::BuildRecord { .. }
                    | InstKind::BuildStr { .. }
                    | InstKind::MakeClosure { .. }
                    | InstKind::MakeClass { .. }
                    | InstKind::MakeEnumVariant { .. }
                    | InstKind::This => {
                        inherently_non_null.insert(dest);
                    }
                    _ => {}
                }
            }
        }
    }

    // 2. Fold block-local IsNull on known inherently non-null or null values
    for block in &mut func.blocks {
        for inst in &mut block.insts {
            if let InstKind::IsNull { operand } = &inst.kind {
                if inherently_non_null.contains(operand) {
                    inst.kind = InstKind::ConstBool(false);
                    changed = true;
                } else if null_literals.contains(operand) {
                    inst.kind = InstKind::ConstBool(true);
                    changed = true;
                }
            }
        }
    }

    // 3. Map each Value to its defining InstKind
    let mut defs: FxHashMap<Value, InstKind> = FxHashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(d) = inst.dest {
                defs.insert(d, inst.kind.clone());
            }
        }
    }

    // 4. Collect branch facts from Terminator::Branch
    let mut block_facts: FxHashMap<BlockId, FxHashMap<Value, NullFact>> = FxHashMap::default();

    for (b_idx, block) in func.blocks.iter().enumerate() {
        let b_id = BlockId(b_idx as u32);
        if let Terminator::Branch {
            cond,
            then_blk,
            else_blk,
            ..
        } = &block.term
        {
            let mut then_facts = FxHashMap::default();
            let mut else_facts = FxHashMap::default();

            if let Some(def) = defs.get(cond) {
                match def {
                    InstKind::IsNull { operand } => {
                        then_facts.insert(*operand, NullFact::Null);
                        else_facts.insert(*operand, NullFact::NonNull);
                    }
                    InstKind::Binary {
                        op: HirBinOp::Eq,
                        lhs,
                        rhs,
                        ..
                    } => {
                        if null_literals.contains(rhs)
                            || matches!(defs.get(rhs), Some(InstKind::ConstNull))
                        {
                            then_facts.insert(*lhs, NullFact::Null);
                            else_facts.insert(*lhs, NullFact::NonNull);
                        } else if null_literals.contains(lhs)
                            || matches!(defs.get(lhs), Some(InstKind::ConstNull))
                        {
                            then_facts.insert(*rhs, NullFact::Null);
                            else_facts.insert(*rhs, NullFact::NonNull);
                        }
                    }
                    InstKind::Binary {
                        op: HirBinOp::Ne,
                        lhs,
                        rhs,
                        ..
                    } => {
                        if null_literals.contains(rhs)
                            || matches!(defs.get(rhs), Some(InstKind::ConstNull))
                        {
                            then_facts.insert(*lhs, NullFact::NonNull);
                            else_facts.insert(*lhs, NullFact::Null);
                        } else if null_literals.contains(lhs)
                            || matches!(defs.get(lhs), Some(InstKind::ConstNull))
                        {
                            then_facts.insert(*rhs, NullFact::NonNull);
                            else_facts.insert(*rhs, NullFact::Null);
                        }
                    }
                    _ => {}
                }
            }

            if func.blocks[then_blk.0 as usize].preds == [b_id] {
                block_facts.entry(*then_blk).or_default().extend(then_facts);
            }
            if func.blocks[else_blk.0 as usize].preds == [b_id] {
                block_facts.entry(*else_blk).or_default().extend(else_facts);
            }
        }
    }

    // 5. Apply block_facts to rewrite downstream IsNull checks
    for (b_id, facts) in &block_facts {
        let block = &mut func.blocks[b_id.0 as usize];
        for inst in &mut block.insts {
            if let InstKind::IsNull { operand } = &inst.kind {
                if let Some(fact) = facts.get(operand) {
                    match fact {
                        NullFact::Null => {
                            inst.kind = InstKind::ConstBool(true);
                            changed = true;
                        }
                        NullFact::NonNull => {
                            inst.kind = InstKind::ConstBool(false);
                            changed = true;
                        }
                    }
                }
            }
        }
    }

    changed
}
