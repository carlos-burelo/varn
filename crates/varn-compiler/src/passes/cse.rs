


























use rustc_hash::FxHashMap;

use crate::hir::{HirBinOp, HirType, HirUnOp};
use crate::ssa::ir::{InstKind, SsaFunc, Value};
use crate::ssa::uses::replace_uses_with_map;

use std::sync::Arc;





#[derive(PartialEq, Eq, Hash)]
enum Key {
    Int(i64),
    FloatBits(u64),
    Bool(bool),
    Str(Arc<str>),
    Char(char),
    BigInt(Arc<str>),
    Null,
    Binary(HirBinOp, HirType, u32, u32),
    Unary(HirUnOp, HirType, u32),
    IsNull(u32),
    IsArray(u32),
    StrLength(u32),
    EnumTag(u32),

    
    FixedField(u32, u16),
    ArrayElem(u32, u32),
    ModuleSlot(u32, u16),
    Global(Arc<str>),
    GlobalIdx(u32),
    NativeGlobalIdx(u32),
    Upvalue(u32),
}

impl Key {
    
    fn reads_memory(&self) -> bool {
        matches!(
            self,
            Key::FixedField(..)
                | Key::ArrayElem(..)
                | Key::ModuleSlot(..)
                | Key::Global(_)
                | Key::GlobalIdx(_)
                | Key::NativeGlobalIdx(_)
                | Key::Upvalue(_)
        )
    }
}

pub fn run(func: &mut SsaFunc) -> bool {
    crate::ssa::verify::recompute_preds(func);
    let mut rewrites: FxHashMap<Value, Value> = FxHashMap::default();

    for block in &func.blocks {
        let mut table: FxHashMap<Key, Value> = FxHashMap::default();

        for inst in &block.insts {
            if !super::dce::is_pure(&inst.kind) {
                table.retain(|k, _| !k.reads_memory());
                continue;
            }
            let Some(dest) = inst.dest else { continue };

            
            
            
            let Some(key) = key_of(&inst.kind, &|v| resolve(&rewrites, v)) else {
                continue;
            };

            match table.get(&key) {
                Some(&existing) => {
                    rewrites.insert(dest, existing);
                }
                None => {
                    table.insert(key, dest);
                }
            }
        }
    }

    replace_uses_with_map(func, &rewrites)
}

pub fn run_global(func: &mut SsaFunc) -> bool {
    crate::ssa::verify::recompute_preds(func);
    let dom = super::cfg::dominators(func);
    let mut uses: Vec<Vec<u32>> = vec![Vec::new(); func.values.len()];
    for (b, block) in func.blocks.iter().enumerate() {
        let mut push = |v: crate::ssa::ir::Value| {
            if let Some(slot) = uses.get_mut(v.0 as usize) {
                if slot.last() != Some(&(b as u32)) {
                    slot.push(b as u32);
                }
            }
        };
        for inst in &block.insts {
            crate::ssa::uses::visit_uses(&inst.kind, &mut push);
        }
        crate::ssa::uses::visit_term_uses(&block.term, &mut push);
    }
    let mut seen: FxHashMap<Key, (Value, usize)> = FxHashMap::default();
    let mut rewrites: FxHashMap<Value, Value> = FxHashMap::default();
    for b in 0..func.blocks.len() {
        for inst in &func.blocks[b].insts {
            let Some(dest) = inst.dest else { continue };
            if !super::dce::is_pure(&inst.kind) {
                continue;
            }
            let Some(key) = key_of(&inst.kind, &|v| resolve(&rewrites, v)) else {
                continue;
            };
            if key.reads_memory() {
                continue;
            }
            if !matches!(key, Key::Binary(..) | Key::Unary(..)) {
                continue;
            }
            match seen.get(&key) {
                Some((existing, def)) => {
                    let dominated = uses.get(dest.0 as usize).is_some_and(|bs| {
                        bs.iter()
                            .all(|u| super::cfg::dominates(&dom, *def, *u as usize))
                    });
                    if dominated {
                        rewrites.insert(dest, *existing);
                    }
                }
                None => {
                    seen.insert(key, (dest, b));
                }
            }
        }
    }
    if rewrites.is_empty() {
        return false;
    }
    replace_uses_with_map(func, &rewrites)
}


fn resolve(rewrites: &FxHashMap<Value, Value>, v: Value) -> u32 {
    let mut cur = v;
    
    
    
    for _ in 0..rewrites.len() + 1 {
        match rewrites.get(&cur) {
            Some(&next) if next != cur => cur = next,
            _ => break,
        }
    }
    cur.0
}

fn key_of(kind: &InstKind, id: &impl Fn(Value) -> u32) -> Option<Key> {
    Some(match kind {
        InstKind::ConstInt(i) => Key::Int(*i),
        InstKind::ConstFloat(f) => Key::FloatBits(f.to_bits()),
        InstKind::ConstBool(b) => Key::Bool(*b),
        InstKind::ConstStr(s) => Key::Str(s.clone()),
        InstKind::ConstChar(c) => Key::Char(*c),
        InstKind::ConstBigInt(i) => Key::BigInt(i.clone()),
        InstKind::ConstNull => Key::Null,

        InstKind::Binary { op, lhs, rhs, ty } => Key::Binary(*op, *ty, id(*lhs), id(*rhs)),
        InstKind::Unary { op, operand, ty } => Key::Unary(*op, *ty, id(*operand)),
        InstKind::IsNull { operand } => Key::IsNull(id(*operand)),
        InstKind::IsArray { operand } => Key::IsArray(id(*operand)),
        InstKind::StrLength { operand } => Key::StrLength(id(*operand)),
        InstKind::GetEnumTag { operand } => Key::EnumTag(id(*operand)),

        InstKind::GetFixedField { object, slot, .. } => Key::FixedField(id(*object), *slot),
        InstKind::ArrayGetIndex { object, index } => Key::ArrayElem(id(*object), id(*index)),
        InstKind::ModuleSlot { object, slot } => Key::ModuleSlot(id(*object), *slot),
        InstKind::LoadGlobal(name) => Key::Global(name.clone()),
        InstKind::LoadGlobalIdx(slot) => Key::GlobalIdx(*slot),
        InstKind::LoadNativeGlobalIdx(slot) => Key::NativeGlobalIdx(*slot),
        InstKind::LoadUpvalue(i) => Key::Upvalue(*i),

        
        
        
        
        
        _ => return None,
    })
}
