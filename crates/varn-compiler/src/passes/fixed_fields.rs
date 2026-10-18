use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::ssa::ir::{InstKind, SsaFunc, Terminator, Value};
use crate::ssa::verify::inst_uses;

pub fn run(func: &mut SsaFunc) -> bool {
    let mut const_ints: FxHashMap<u32, i64> = FxHashMap::default();

    let mut obj_literals: FxHashMap<u32, Vec<(Arc<str>, Value)>> = FxHashMap::default();

    let mut tuple_literals: FxHashMap<u32, Vec<Value>> = FxHashMap::default();

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
                    InstKind::ConstFloat(_)
                    | InstKind::ConstBool(_)
                    | InstKind::ConstStr(_)
                    | InstKind::ConstChar(_)
                    | InstKind::ConstDecimal(_)
                    | InstKind::ConstBigInt(_)
                    | InstKind::ConstNull
                    | InstKind::Binary { .. }
                    | InstKind::Unary { .. }
                    | InstKind::LoadGlobal(_)
                    | InstKind::LoadGlobalIdx(_)
                    | InstKind::LoadNativeGlobalIdx(_)
                    | InstKind::LoadUpvalue(_)
                    | InstKind::StoreGlobal { .. }
                    | InstKind::StoreGlobalIdx { .. }
                    | InstKind::StoreUpvalue { .. }
                    | InstKind::Call { .. }
                    | InstKind::AllocInstance { .. }
                    | InstKind::SelfCall { .. }
                    | InstKind::GetProperty { .. }
                    | InstKind::GetFixedField { .. }
                    | InstKind::GetIndex { .. }
                    | InstKind::ArrayGetIndex { .. }
                    | InstKind::BytesGetIndex { .. }
                    | InstKind::MapGetIndex { .. }
                    | InstKind::SetProperty { .. }
                    | InstKind::SetFixedField { .. }
                    | InstKind::SetIndex { .. }
                    | InstKind::ArraySetIndex { .. }
                    | InstKind::BytesSetIndex { .. }
                    | InstKind::MapSetIndex { .. }
                    | InstKind::ArrayPush { .. }
                    | InstKind::ObjectMerge { .. }
                    | InstKind::MethodCall { .. }
                    | InstKind::IsNull { .. }
                    | InstKind::Cast { .. }
                    | InstKind::Convert { .. }
                    | InstKind::BuildMap { .. }
                    | InstKind::ObjectRest { .. }
                    | InstKind::ToString { .. }
                    | InstKind::BuildStr { .. }
                    | InstKind::MakeClosure { .. }
                    | InstKind::LoadCaptured { .. }
                    | InstKind::StoreCaptured { .. }
                    | InstKind::MakeClass { .. }
                    | InstKind::DeclareLayout { .. }
                    | InstKind::DefineStatic { .. }
                    | InstKind::DefineMethod { .. }
                    | InstKind::DefineAccessor { .. }
                    | InstKind::MakeEnumVariant { .. }
                    | InstKind::Try { .. }
                    | InstKind::PopTry
                    | InstKind::CatchParam { .. }
                    | InstKind::CloseUpvalues { .. }
                    | InstKind::Dispose { .. }
                    | InstKind::LoadModule { .. }
                    | InstKind::StoreModuleSlot { .. }
                    | InstKind::Await { .. }
                    | InstKind::Spawn { .. }
                    | InstKind::Yield { .. }
                    | InstKind::IntrinsicCall { .. }
                    | InstKind::CallNativeOp { .. }
                    | InstKind::AssertNotNull { .. }
                    | InstKind::GetPropertyMaybe { .. }
                    | InstKind::ModuleSlot { .. }
                    | InstKind::GetEnumTag { .. }
                    | InstKind::IsArray { .. }
                    | InstKind::StrLength { .. }
                    | InstKind::ArrayLength { .. }
                    | InstKind::BytesLength { .. }
                    | InstKind::This
                    | InstKind::Range { .. }
                    | InstKind::ObjectKeys { .. }
                    | InstKind::GetSymbol { .. }
                    | InstKind::IterCall { .. }
                    | InstKind::GetSuper { .. }
                    | InstKind::SuperCall { .. }
                    | InstKind::SuperMethodCall { .. }
                    | InstKind::ExtensionCall { .. }
                    | InstKind::CallSpread { .. }
                    | InstKind::BuildArraySpread { .. }
                    | InstKind::BuildObjectSpread { .. } => {}
                }
            }
        }
    }

    if obj_literals.is_empty() && tuple_literals.is_empty() && array_literals.is_empty() {
        return false;
    }

    for block in &func.blocks {
        for inst in &block.insts {
            if let InstKind::GetProperty { object, .. } = &inst.kind
                && obj_literals.contains_key(&object.0)
            {
                continue;
            }
            if let InstKind::GetIndex { object, index }
            | InstKind::ArrayGetIndex { object, index } = &inst.kind
            {
                let elems = tuple_literals
                    .get(&object.0)
                    .or_else(|| array_literals.get(&object.0));
                if let (Some(elems), Some(&idx)) = (elems, const_ints.get(&index.0))
                    && idx >= 0
                    && (idx as usize) < elems.len()
                {
                    continue;
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
            Terminator::Return(_) | Terminator::Unreachable => {}
        }
    }

    if obj_literals.is_empty() && tuple_literals.is_empty() && array_literals.is_empty() {
        return false;
    }

    let mut forwards: Vec<(Value, Value)> = Vec::new();
    let mut dead_reads: FxHashSet<Value> = FxHashSet::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let (Some(dest), InstKind::GetProperty { object, name }) = (inst.dest, &inst.kind)
                && let Some(pairs) = obj_literals.get(&object.0)
                && let Some((_, v)) = pairs.iter().find(|(k, _)| k == name)
            {
                forwards.push((dest, *v));
                dead_reads.insert(dest);
            }
            if let (
                Some(dest),
                InstKind::GetIndex { object, index } | InstKind::ArrayGetIndex { object, index },
            ) = (inst.dest, &inst.kind)
            {
                if let Some(elems) = tuple_literals.get(&object.0)
                    && let Some(&idx) = const_ints.get(&index.0)
                    && idx >= 0
                    && (idx as usize) < elems.len()
                {
                    forwards.push((dest, elems[idx as usize]));
                }
                if let Some(elems) = array_literals.get(&object.0)
                    && let Some(&idx) = const_ints.get(&index.0)
                    && idx >= 0
                    && (idx as usize) < elems.len()
                {
                    let elem = elems[idx as usize];
                    if func.value_ty(elem) == func.value_ty(dest) {
                        forwards.push((dest, elem));
                        dead_reads.insert(dest);
                    }
                }
            }
        }
    }

    let changed = !forwards.is_empty();

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
