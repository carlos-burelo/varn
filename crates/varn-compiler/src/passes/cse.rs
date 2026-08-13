//! Local common-subexpression elimination.
//!
//! Scope is one block at a time, deliberately. The redundancy this pass was
//! written for is intra-block — a loop body that reads `p.x` twice and
//! rematerializes the same literal at every use — and a block-local table
//! needs no aliasing theory to be sound: the scan is linear, so "has anything
//! written since?" is answered by having seen the write.
//!
//! Extending the table across the dominator tree would be unsound for loads
//! without memory SSA (a block can dominate another and still have a store on
//! the path between them) and would only extend live ranges for the pure
//! arithmetic where it *is* sound. Both are decisions to revisit with a
//! measurement, not by default.
//!
//! Two tables, because they have different lifetimes:
//!
//! * `values` — results determined entirely by the operand SSA values
//!   (constants, typed arithmetic, type tests). Nothing in a block can
//!   invalidate these.
//! * `loads` — results that read memory (`GetFixedField`, `ArrayGetIndex`,
//!   globals, upvalues). Dropped wholesale at the first instruction that is
//!   not provably pure, since a store or any user code could have changed
//!   what a repeat read would see.
//!
//! Purity comes from [`super::dce::is_pure`] — the same allow-list DCE
//! deletes by, so the two passes cannot disagree about what runs user code.

use rustc_hash::FxHashMap;

use crate::hir::{HirBinOp, HirType, HirUnOp};
use crate::ssa::ir::{InstKind, SsaFunc, Value};
use crate::ssa::uses::replace_uses_with_map;

use std::rc::Rc;

/// Structural identity of a redundant-eliminable expression.
///
/// Operand ids are stored raw so the key is cheap to hash; floats go in as
/// bits, which keeps `NaN` and `-0.0` from being conflated with anything.
#[derive(PartialEq, Eq, Hash)]
enum Key {
    Int(i64),
    FloatBits(u64),
    Bool(bool),
    Str(Rc<str>),
    Char(char),
    BigInt(i128),
    Null,
    Binary(HirBinOp, HirType, u32, u32),
    Unary(HirUnOp, HirType, u32),
    IsNull(u32),
    IsArray(u32),
    EnumTag(u32),

    // Memory-dependent from here down; see `loads`.
    FixedField(u32, u16),
    ArrayElem(u32, u32),
    ModuleSlot(u32, u16),
    Global(Rc<str>),
    Upvalue(u32),
}

impl Key {
    /// Whether this key's value can be invalidated by a store or user code.
    fn reads_memory(&self) -> bool {
        matches!(
            self,
            Key::FixedField(..)
                | Key::ArrayElem(..)
                | Key::ModuleSlot(..)
                | Key::Global(_)
                | Key::Upvalue(_)
        )
    }
}

pub fn run(func: &mut SsaFunc) -> bool {
    let mut rewrites: FxHashMap<Value, Value> = FxHashMap::default();

    for block in &func.blocks {
        let mut table: FxHashMap<Key, Value> = FxHashMap::default();

        for inst in &block.insts {
            if !super::dce::is_pure(&inst.kind) {
                table.retain(|k, _| !k.reads_memory());
                continue;
            }
            let Some(dest) = inst.dest else { continue };

            // Key on the operands' canonical ids, so a value this pass has
            // already redirected does not read as a different expression —
            // otherwise `a.f + 1` twice would need a second fixpoint round.
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

/// Follows a value through the rewrites recorded so far.
fn resolve(rewrites: &FxHashMap<Value, Value>, v: Value) -> u32 {
    let mut cur = v;
    // Rewrites only ever point backwards to an earlier definition, so the
    // chain is finite; the bound is belt-and-braces against a future edit
    // that inserts one the other way round.
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
        InstKind::ConstBigInt(i) => Key::BigInt(*i),
        InstKind::ConstNull => Key::Null,

        InstKind::Binary { op, lhs, rhs, ty } => Key::Binary(*op, *ty, id(*lhs), id(*rhs)),
        InstKind::Unary { op, operand, ty } => Key::Unary(*op, *ty, id(*operand)),
        InstKind::IsNull { operand } => Key::IsNull(id(*operand)),
        InstKind::IsArray { operand } => Key::IsArray(id(*operand)),
        InstKind::GetEnumTag { operand } => Key::EnumTag(id(*operand)),

        InstKind::GetFixedField { object, slot } => Key::FixedField(id(*object), *slot),
        InstKind::ArrayGetIndex { object, index } => Key::ArrayElem(id(*object), id(*index)),
        InstKind::ModuleSlot { object, slot } => Key::ModuleSlot(id(*object), *slot),
        InstKind::LoadGlobal(name) => Key::Global(name.clone()),
        InstKind::LoadUpvalue(i) => Key::Upvalue(*i),

        // `ConstDecimal` is left out on purpose: `Decimal` compares equal
        // across differing scales (`1.0` vs `1.00`), which are distinct
        // values once they reach arithmetic, so it is not a safe hash key.
        // Everything else pure but not listed is either an allocation (whose
        // identity matters) or not worth a table entry.
        _ => return None,
    })
}
