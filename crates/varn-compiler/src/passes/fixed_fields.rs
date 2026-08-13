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
    // ConstInt definitions in function: SSA value -> i64
    let mut const_ints: FxHashMap<u32, i64> = FxHashMap::default();
    // Object/Record-literal defs: SSA value -> (key, value) pairs in slot order.
    let mut obj_literals: FxHashMap<u32, Vec<(Rc<str>, Value)>> = FxHashMap::default();
    // Tuple-literal defs: SSA value -> elements in index order.
    let mut tuple_literals: FxHashMap<u32, Vec<Value>> = FxHashMap::default();

    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(d) = inst.dest {
                match &inst.kind {
                    InstKind::ConstInt(n) => {
                        const_ints.insert(d.0, *n);
                    }
                    InstKind::BuildObject { pairs } | InstKind::BuildRecord { pairs } => {
                        let mut seen: Vec<_> = pairs.iter().map(|(k, _)| k.clone()).collect();
                        seen.sort();
                        seen.dedup();
                        if seen.len() == pairs.len() {
                            obj_literals.insert(d.0, pairs.clone());
                        }
                    }
                    InstKind::BuildTuple { elements } => {
                        tuple_literals.insert(d.0, elements.clone());
                    }
                    _ => {}
                }
            }
        }
    }

    if obj_literals.is_empty() && tuple_literals.is_empty() {
        return false;
    }

    // Disqualify escaping or shape-mutating uses.
    for block in &func.blocks {
        for inst in &block.insts {
            if let InstKind::GetProperty { object, .. } = &inst.kind {
                if obj_literals.contains_key(&object.0) {
                    continue;
                }
            }
            if let InstKind::GetIndex { object, index }
            | InstKind::ArrayGetIndex { object, index } = &inst.kind
            {
                if tuple_literals.contains_key(&object.0) {
                    if let Some(&idx) = const_ints.get(&index.0) {
                        if let Some(elems) = tuple_literals.get(&object.0) {
                            if idx >= 0 && (idx as usize) < elems.len() {
                                continue;
                            }
                        }
                    }
                }
            }

            for u in inst_uses(&inst.kind) {
                obj_literals.remove(&u.0);
                tuple_literals.remove(&u.0);
            }
        }
        match &block.term {
            Terminator::Return(Some(v)) | Terminator::Throw(v) => {
                obj_literals.remove(&v.0);
                tuple_literals.remove(&v.0);
            }
            Terminator::Branch {
                cond,
                then_args,
                else_args,
                ..
            } => {
                obj_literals.remove(&cond.0);
                tuple_literals.remove(&cond.0);
                for a in then_args.iter().chain(else_args) {
                    obj_literals.remove(&a.0);
                    tuple_literals.remove(&a.0);
                }
            }
            Terminator::Jump { args, .. } => {
                for a in args {
                    obj_literals.remove(&a.0);
                    tuple_literals.remove(&a.0);
                }
            }
            _ => {}
        }
    }

    if obj_literals.is_empty() && tuple_literals.is_empty() {
        return false;
    }

    // Forward each resolvable read to the stored value.
    let mut forwards: Vec<(Value, Value)> = Vec::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let (Some(dest), InstKind::GetProperty { object, name }) = (inst.dest, &inst.kind) {
                if let Some(pairs) = obj_literals.get(&object.0) {
                    if let Some((_, v)) = pairs.iter().find(|(k, _)| k == name) {
                        forwards.push((dest, *v));
                    }
                }
            }
            if let (
                Some(dest),
                InstKind::GetIndex { object, index } | InstKind::ArrayGetIndex { object, index },
            ) = (inst.dest, &inst.kind)
            {
                if let Some(elems) = tuple_literals.get(&object.0) {
                    if let Some(&idx) = const_ints.get(&index.0) {
                        if idx >= 0 && (idx as usize) < elems.len() {
                            forwards.push((dest, elems[idx as usize]));
                        }
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
