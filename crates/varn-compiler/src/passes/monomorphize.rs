//! Generic monomorphization and type-specialization pass.
//!
//! Specializes generic container indexing (`GetIndex`/`SetIndex`) to monomorphic
//! array operations (`ArrayGetIndex`/`ArraySetIndex`) when static type metadata or
//! SSA allocation sources confirm array layout.

use crate::hir::HirType;
use crate::ssa::ir::{InstKind, SsaFunc};
use rustc_hash::FxHashSet;

pub fn run(func: &mut SsaFunc) -> bool {
    let mut changed = false;
    let mut array_values: FxHashSet<u32> = FxHashSet::default();
    let mut map_values: FxHashSet<u32> = FxHashSet::default();

    // 1. Collect all SSA values that are known arrays or maps via type or constructor
    for (i, vdef) in func.values.iter().enumerate() {
        if matches!(vdef.ty, HirType::Array(_)) {
            array_values.insert(i as u32);
        } else if matches!(vdef.ty, HirType::Map(_, _)) {
            map_values.insert(i as u32);
        }
    }

    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(d) = inst.dest {
                if matches!(inst.kind, InstKind::BuildArray { .. }) {
                    array_values.insert(d.0);
                } else if matches!(inst.kind, InstKind::BuildMap { .. }) {
                    map_values.insert(d.0);
                }
            }
        }
    }

    if array_values.is_empty() && map_values.is_empty() {
        return false;
    }

    // 2. Promote generic GetIndex/SetIndex to specialized ArrayGetIndex/ArraySetIndex or MapGetIndex/MapSetIndex
    for block in &mut func.blocks {
        for inst in &mut block.insts {
            match &inst.kind {
                InstKind::GetIndex { object, index } if array_values.contains(&object.0) => {
                    inst.kind = InstKind::ArrayGetIndex {
                        object: *object,
                        index: *index,
                    };
                    changed = true;
                }
                InstKind::SetIndex {
                    object,
                    index,
                    value,
                } if array_values.contains(&object.0) => {
                    inst.kind = InstKind::ArraySetIndex {
                        object: *object,
                        index: *index,
                        value: *value,
                    };
                    changed = true;
                }
                InstKind::GetIndex { object, index } if map_values.contains(&object.0) => {
                    inst.kind = InstKind::MapGetIndex {
                        object: *object,
                        index: *index,
                    };
                    changed = true;
                }
                InstKind::SetIndex {
                    object,
                    index,
                    value,
                } if map_values.contains(&object.0) => {
                    inst.kind = InstKind::MapSetIndex {
                        object: *object,
                        index: *index,
                        value: *value,
                    };
                    changed = true;
                }
                _ => {}
            }
        }
    }

    changed
}
