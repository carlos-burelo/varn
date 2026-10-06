use rustc_hash::{FxHashMap, FxHashSet};

use crate::ssa::ir::{InstKind, SsaFunc, Terminator, Value};
use crate::ssa::verify::inst_uses;

struct Site {
    stores: FxHashMap<u16, Value>,
    store_block: usize,
}

pub fn run(func: &mut SsaFunc) -> bool {
    let mut sites: FxHashMap<u32, Site> = FxHashMap::default();
    let mut escaped: FxHashSet<u32> = FxHashSet::default();
    let mut reads: Vec<(Value, u32, u16)> = Vec::new();

    for (bi, block) in func.blocks.iter().enumerate() {
        let mut read_here: FxHashSet<u32> = FxHashSet::default();
        for inst in &block.insts {
            match (&inst.kind, inst.dest) {
                (InstKind::AllocInstance { .. }, Some(d)) => {
                    sites.insert(
                        d.0,
                        Site {
                            stores: FxHashMap::default(),
                            store_block: bi,
                        },
                    );
                    continue;
                }
                (
                    InstKind::SetFixedField {
                        object,
                        value,
                        slot,
                        ..
                    },
                    _,
                ) if sites.contains_key(&object.0) => {
                    escaped.insert(value.0);
                    let site = sites.get_mut(&object.0).expect("a site");
                    let in_order = site.store_block == bi && !read_here.contains(&object.0);
                    if !in_order || site.stores.insert(*slot, *value).is_some() {
                        escaped.insert(object.0);
                    }
                    continue;
                }
                (InstKind::GetFixedField { object, slot, .. }, Some(d))
                    if sites.contains_key(&object.0) =>
                {
                    read_here.insert(object.0);
                    reads.push((d, object.0, *slot));
                    continue;
                }
                _ => {}
            }
            for u in inst_uses(&inst.kind) {
                escaped.insert(u.0);
            }
        }
        match &block.term {
            Terminator::Return(Some(v)) | Terminator::Throw(v) => {
                escaped.insert(v.0);
            }
            Terminator::Branch {
                cond,
                then_args,
                else_args,
                ..
            } => {
                escaped.insert(cond.0);
                escaped.extend(then_args.iter().chain(else_args).map(|a| a.0));
            }
            Terminator::Jump { args, .. } => escaped.extend(args.iter().map(|a| a.0)),
            _ => {}
        }
    }
    sites.retain(|v, _| !escaped.contains(v));

    let mut forwards: Vec<(Value, Value, u32)> = Vec::new();
    for &(dest, site, slot) in &reads {
        let Some(s) = sites.get(&site) else { continue };
        match s.stores.get(&slot) {
            Some(&stored) if func.value_ty(stored) == func.value_ty(dest) => {
                forwards.push((dest, stored, site))
            }
            _ => {
                sites.remove(&site);
            }
        }
    }
    if sites.is_empty() {
        return false;
    }
    let mut to: FxHashMap<Value, Value> = forwards
        .into_iter()
        .filter(|(_, _, site)| sites.contains_key(site))
        .map(|(dest, stored, _)| (dest, stored))
        .collect();
    let keys: Vec<Value> = to.keys().copied().collect();
    for k in keys {
        let mut v = to[&k];
        while let Some(&w) = to.get(&v) {
            if w == v {
                break;
            }
            v = w;
        }
        to.insert(k, v);
    }
    for (dest, stored) in to {
        func.replace_all_uses(dest, stored);
    }
    for block in &mut func.blocks {
        block.insts.retain(|inst| match (&inst.kind, inst.dest) {
            (InstKind::AllocInstance { .. }, Some(d)) => !sites.contains_key(&d.0),
            (InstKind::SetFixedField { object, .. }, _)
            | (InstKind::GetFixedField { object, .. }, _) => !sites.contains_key(&object.0),
            _ => true,
        });
    }
    true
}
