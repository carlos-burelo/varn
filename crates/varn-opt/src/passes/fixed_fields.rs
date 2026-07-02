//! Devirtualize property reads on locally-built object literals.
//!
//! `BuildObject` produces an object whose shape (key -> slot order) is known
//! at compile time. As long as that object's shape cannot change and the
//! object does not escape the function, every `GetProperty` on it with a
//! known key can load the slot directly (`GetFixedField`), skipping the
//! inline-cache machinery entirely.
//!
//! Disqualification is conservative: any use of the literal other than a
//! `GetProperty` on it (call argument, store into another object, return,
//! branch argument, `SetProperty`, ...) drops it from consideration, since
//! such a use could add keys or let the object outlive our view of it.

use std::rc::Rc;

use rustc_hash::FxHashMap;

use crate::ssa::ir::{InstKind, SsaFunc, Terminator};
use crate::ssa::verify::inst_uses;

pub fn run(func: &mut SsaFunc) -> bool {
    // Object-literal defs: SSA value -> keys in slot order.
    let mut literals: FxHashMap<u32, Vec<Rc<str>>> = FxHashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let (Some(d), InstKind::BuildObject { pairs }) = (inst.dest, &inst.kind) {
                let keys: Vec<Rc<str>> = pairs.iter().map(|(k, _)| k.clone()).collect();
                // Duplicate keys collapse into one runtime slot; skip them.
                let mut seen = keys.clone();
                seen.sort();
                seen.dedup();
                if seen.len() == keys.len() {
                    literals.insert(d.0, keys);
                }
            }
        }
    }
    if literals.is_empty() {
        return false;
    }

    // Disqualify escaping or shape-mutating uses.
    for block in &func.blocks {
        for inst in &block.insts {
            if let InstKind::GetProperty { object, .. } = &inst.kind {
                if literals.contains_key(&object.0) {
                    continue;
                }
            }
            for u in inst_uses(&inst.kind) {
                literals.remove(&u.0);
            }
        }
        match &block.term {
            Terminator::Return(Some(v)) | Terminator::Throw(v) => {
                literals.remove(&v.0);
            }
            Terminator::Branch {
                cond,
                then_args,
                else_args,
                ..
            } => {
                literals.remove(&cond.0);
                for a in then_args.iter().chain(else_args) {
                    literals.remove(&a.0);
                }
            }
            Terminator::Jump { args, .. } => {
                for a in args {
                    literals.remove(&a.0);
                }
            }
            _ => {}
        }
    }
    if literals.is_empty() {
        return false;
    }

    let mut changed = false;
    for block in &mut func.blocks {
        for inst in &mut block.insts {
            if let InstKind::GetProperty { object, name } = &inst.kind {
                if let Some(keys) = literals.get(&object.0) {
                    if let Some(slot) = keys.iter().position(|k| k == name) {
                        inst.kind = InstKind::GetFixedField {
                            object: *object,
                            slot: slot as u16,
                        };
                        changed = true;
                    }
                }
            }
        }
    }
    changed
}
