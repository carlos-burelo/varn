//! Scalar replacement and devirtualization of locally-built object literals.
//!
//! `BuildObject` produces an object whose shape (key -> slot order) is known
//! at compile time. As long as that object's shape cannot change and the
//! object does not escape the function, every `GetProperty` on it with a
//! known key reads a value the compiler already has in SSA form — so the
//! read is forwarded to that value directly. Reads the pass forwards become
//! dead, and once every read of a literal is forwarded the `BuildObject`
//! itself is dead too; DCE deletes both, eliminating the allocation.
//!
//! Reads whose key is not part of the literal's shape are left as
//! `GetProperty` (they resolve through the normal runtime path), which keeps
//! the allocation alive.
//!
//! Disqualification is conservative: any use of the literal other than a
//! `GetProperty` on it (call argument, store into another object, return,
//! branch argument, `SetProperty`, ...) drops it from consideration, since
//! such a use could add keys or let the object outlive our view of it.

use std::rc::Rc;

use rustc_hash::FxHashMap;

use crate::ssa::ir::{InstKind, SsaFunc, Terminator, Value};
use crate::ssa::verify::inst_uses;

pub fn run(func: &mut SsaFunc) -> bool {
    // Object-literal defs: SSA value -> (key, value) pairs in slot order.
    let mut literals: FxHashMap<u32, Vec<(Rc<str>, Value)>> = FxHashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let (Some(d), InstKind::BuildObject { pairs }) = (inst.dest, &inst.kind) {
                // Duplicate keys collapse into one runtime slot; skip them.
                let mut seen: Vec<_> = pairs.iter().map(|(k, _)| k.clone()).collect();
                seen.sort();
                seen.dedup();
                if seen.len() == pairs.len() {
                    literals.insert(d.0, pairs.clone());
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

    // Forward each resolvable read to the stored value. The read becomes
    // dead and DCE removes it; a literal whose reads all forwarded loses
    // its last use and DCE removes the allocation on the next iteration.
    let mut forwards: Vec<(Value, Value)> = Vec::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let (Some(dest), InstKind::GetProperty { object, name }) = (inst.dest, &inst.kind) {
                if let Some(pairs) = literals.get(&object.0) {
                    if let Some((_, v)) = pairs.iter().find(|(k, _)| k == name) {
                        forwards.push((dest, *v));
                    }
                }
            }
        }
    }

    let changed = !forwards.is_empty();
    for (dest, value) in forwards {
        func.replace_all_uses(dest, value);
    }
    changed
}
