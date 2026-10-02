//! Scalar replacement and devirtualization of locally-built object literals.
//!
//! `BuildObject` produces an object whose shape (key -> slot order) is known
//! at compile time. As long as that object's shape cannot change and the
//! object does not escape the function, every `GetProperty` on it with a
//! known key reads a value the compiler already has in SSA form — so the
//! read is forwarded to that value directly. A forwarded read runs no user
//! code (a fresh literal carries no class, and no Varn-level operation can
//! attach one without using the object — which disqualifies it), resolves a
//! key its shape is known to hold (so it cannot throw), and leaves no trace
//! in any cache the program can observe — so this pass deletes the read
//! itself. Once every read of a literal is forwarded, the `BuildObject` is
//! dead and DCE deletes the allocation.
//!
//! Reads whose key is not part of the literal's shape are left as
//! `GetProperty` (they resolve through the normal runtime path), which keeps
//! the allocation alive.
//!
//! Disqualification is conservative: any use of the literal other than a
//! `GetProperty` on it (call argument, store into another object, return,
//! branch argument, `SetProperty`, ...) drops it from consideration, since
//! such a use could add keys or let the object outlive our view of it.
//!
//! Array literals follow the same rule with constant in-bounds indices: a
//! `BuildArray` read only through `GetIndex`/`ArrayGetIndex` with a literal
//! index never escapes, grows or is written (`push`, `length`, `SetIndex` and
//! every other use disqualify it), so each read is the element itself. The
//! forward is skipped unless the element and the read share an SSA type, so
//! a representation change on store (`Array<float>` holding an `int`
//! literal) is never bypassed.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::ssa::ir::{InstKind, SsaFunc, Terminator, Value};
use crate::ssa::verify::inst_uses;

pub fn run(func: &mut SsaFunc) -> bool {
    // ConstInt definitions in function: SSA value -> i64
    let mut const_ints: FxHashMap<u32, i64> = FxHashMap::default();
    // Object/Record-literal defs: SSA value -> (key, value) pairs in slot order.
    let mut obj_literals: FxHashMap<u32, Vec<(Arc<str>, Value)>> = FxHashMap::default();
    // Tuple-literal defs: SSA value -> elements in index order.
    let mut tuple_literals: FxHashMap<u32, Vec<Value>> = FxHashMap::default();
    // Array-literal defs: SSA value -> elements in index order.
    let mut array_literals: FxHashMap<u32, Vec<Value>> = FxHashMap::default();

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
                    InstKind::BuildArray { elements } => {
                        array_literals.insert(d.0, elements.clone());
                    }
                    _ => {}
                }
            }
        }
    }

    if obj_literals.is_empty() && tuple_literals.is_empty() && array_literals.is_empty() {
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
                let elems = tuple_literals
                    .get(&object.0)
                    .or_else(|| array_literals.get(&object.0));
                if let (Some(elems), Some(&idx)) = (elems, const_ints.get(&index.0)) {
                    if idx >= 0 && (idx as usize) < elems.len() {
                        continue;
                    }
                }
            }

            for u in inst_uses(&inst.kind) {
                obj_literals.remove(&u.0);
                tuple_literals.remove(&u.0);
                array_literals.remove(&u.0);
            }
        }
        match &block.term {
            Terminator::Return(Some(v)) | Terminator::Throw(v) => {
                obj_literals.remove(&v.0);
                tuple_literals.remove(&v.0);
                array_literals.remove(&v.0);
            }
            Terminator::Branch {
                cond,
                then_args,
                else_args,
                ..
            } => {
                obj_literals.remove(&cond.0);
                tuple_literals.remove(&cond.0);
                array_literals.remove(&cond.0);
                for a in then_args.iter().chain(else_args) {
                    obj_literals.remove(&a.0);
                    tuple_literals.remove(&a.0);
                    array_literals.remove(&a.0);
                }
            }
            Terminator::Jump { args, .. } => {
                for a in args {
                    obj_literals.remove(&a.0);
                    tuple_literals.remove(&a.0);
                    array_literals.remove(&a.0);
                }
            }
            _ => {}
        }
    }

    if obj_literals.is_empty() && tuple_literals.is_empty() && array_literals.is_empty() {
        return false;
    }

    // Forward each resolvable read to the stored value, and delete the
    // `GetProperty` reads: on a known literal they are effect-free (see
    // module docs), and leaving them for DCE would pin the allocation
    // forever — `GetProperty` is impure in general (class getters), so DCE
    // correctly refuses it without the literal knowledge that only lives in
    // this pass. Tuple reads stay for DCE, which already deletes them: the
    // statically-typed form is pure, and the dynamic form keeps the
    // allocation alive by design.
    let mut forwards: Vec<(Value, Value)> = Vec::new();
    let mut dead_reads: FxHashSet<Value> = FxHashSet::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let (Some(dest), InstKind::GetProperty { object, name }) = (inst.dest, &inst.kind) {
                if let Some(pairs) = obj_literals.get(&object.0) {
                    if let Some((_, v)) = pairs.iter().find(|(k, _)| k == name) {
                        forwards.push((dest, *v));
                        dead_reads.insert(dest);
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
                if let Some(elems) = array_literals.get(&object.0) {
                    if let Some(&idx) = const_ints.get(&index.0) {
                        if idx >= 0 && (idx as usize) < elems.len() {
                            let elem = elems[idx as usize];
                            if func.value_ty(elem) == func.value_ty(dest) {
                                forwards.push((dest, elem));
                                dead_reads.insert(dest);
                            }
                        }
                    }
                }
            }
        }
    }

    let changed = !forwards.is_empty();
    // Transitively resolve chained forwards before rewriting: a forwarded
    // value can itself be another forwarded read's destination
    // (`v12 -> v3 -> v0` when a tuple holds a literal's field), and the
    // intermediate def is deleted below — capturing it stale would leave a
    // use of an undefined value. The map is acyclic (every edge points at a
    // pre-existing value), so this terminates.
    let mut fwd_map: FxHashMap<Value, Value> = forwards.into_iter().collect();
    let keys: Vec<Value> = fwd_map.keys().copied().collect();
    for k in keys {
        let mut v = fwd_map[&k];
        loop {
            match fwd_map.get(&v) {
                Some(&w) if w != v => v = w,
                _ => break,
            }
        }
        fwd_map.insert(k, v);
    }
    for (dest, value) in fwd_map {
        func.replace_all_uses(dest, value);
    }
    if !dead_reads.is_empty() {
        for block in &mut func.blocks {
            block
                .insts
                .retain(|inst| inst.dest.is_none_or(|d| !dead_reads.contains(&d)));
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::HirType;
    use crate::ssa::ir::{Block, BlockId, Inst, SsaFunc, Terminator, ValueDef};

    fn inst(dest: Option<u32>, kind: InstKind) -> Inst {
        Inst {
            dest: dest.map(Value),
            kind,
            line: 0,
        }
    }

    /// `bench_now` en miniatura: un literal construido y leído dentro de un
    /// cuerpo de loop debe redirigirse al valor SSA y soltar el alloc.
    #[test]
    fn loop_body_literal_read_forwards() {
        // v0 = i (parámetro de bloque), v1 = i+1, v2 = objeto, v3 = x.a.
        let body = Block {
            params: vec![Value(0)],
            insts: vec![
                inst(
                    Some(4),
                    InstKind::Binary {
                        op: crate::hir::HirBinOp::Add,
                        lhs: Value(0),
                        rhs: Value(0),
                        ty: HirType::Int,
                    },
                ),
                inst(
                    Some(2),
                    InstKind::BuildObject {
                        pairs: vec![(Arc::from("a"), Value(0)), (Arc::from("b"), Value(4))],
                    },
                ),
                inst(
                    Some(3),
                    InstKind::GetProperty {
                        object: Value(2),
                        name: Arc::from("a"),
                    },
                ),
                inst(
                    None,
                    InstKind::StoreGlobal {
                        name: Arc::from("sum"),
                        value: Value(3),
                    },
                ),
            ],
            term: Terminator::Jump {
                target: BlockId(0),
                args: vec![Value(0)],
            },
            term_line: 0,
            preds: vec![BlockId(0)],
        };
        let entry = Block {
            params: vec![],
            insts: vec![],
            term: Terminator::Jump {
                target: BlockId(1),
                args: vec![Value(0)],
            },
            term_line: 0,
            preds: vec![],
        };
        let mut func = SsaFunc {
            name: Arc::from("probe"),
            entry: BlockId(0),
            blocks: vec![entry, body],
            values: vec![ValueDef { ty: HirType::Int }; 5],
            is_async: false,
            is_generator: false,
        };
        assert!(run(&mut func), "literal read should forward");
        // El store ahora usa `i` directo, no el resultado del GetProperty.
        let store = func.blocks[1]
            .insts
            .iter()
            .find_map(|i| match &i.kind {
                InstKind::StoreGlobal { value, .. } => Some(*value),
                _ => None,
            })
            .expect("store survives");
        assert_eq!(store, Value(0));
    }

    /// throughATuple en miniatura: una tupla que guarda el campo de un
    /// literal (`v12 -> v3 -> v0`) debe resolverse al valor final — capturar
    /// el intermedio obsoleto y borrar su def deja un uso indefinido.
    #[test]
    fn chained_tuple_forward_resolves_transitively() {
        // v0 = 1, v2 = objeto, v3 = x.id, v8 = tupla, v12 = t[0] -> v0.
        let body = Block {
            params: vec![],
            insts: vec![
                inst(Some(0), InstKind::ConstInt(1)),
                inst(
                    Some(2),
                    InstKind::BuildObject {
                        pairs: vec![(Arc::from("id"), Value(0))],
                    },
                ),
                inst(
                    Some(3),
                    InstKind::GetProperty {
                        object: Value(2),
                        name: Arc::from("id"),
                    },
                ),
                inst(
                    Some(8),
                    InstKind::BuildTuple {
                        elements: vec![Value(3)],
                    },
                ),
                inst(Some(11), InstKind::ConstInt(0)),
                inst(
                    Some(12),
                    InstKind::GetIndex {
                        object: Value(8),
                        index: Value(11),
                    },
                ),
                inst(
                    None,
                    InstKind::StoreGlobal {
                        name: Arc::from("sum"),
                        value: Value(12),
                    },
                ),
            ],
            term: Terminator::Return(None),
            term_line: 0,
            preds: vec![],
        };
        let mut func = SsaFunc {
            name: Arc::from("probe"),
            entry: BlockId(0),
            blocks: vec![body],
            values: vec![ValueDef { ty: HirType::Int }; 13],
            is_async: false,
            is_generator: false,
        };
        assert!(run(&mut func), "chained read should forward");
        // El store usa el 1 original; el GetProperty intermedio se borró sin
        // dejar usos colgando (el verificador lo confirma en e2e).
        let store = func.blocks[0]
            .insts
            .iter()
            .find_map(|i| match &i.kind {
                InstKind::StoreGlobal { value, .. } => Some(*value),
                _ => None,
            })
            .expect("store survives");
        assert_eq!(store, Value(0));
        assert!(
            !func.blocks[0]
                .insts
                .iter()
                .any(|i| i.dest == Some(Value(3))),
            "chained intermediate read must be deleted"
        );
    }
}
