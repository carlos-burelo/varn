use rustc_hash::FxHashMap;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::OnceLock;
use varn_core::op_id::{KnownOp, CORE_MODULE, FIRST_REGISTRY_ID};
use varn_types::{NativeCtx, NativeOpEntry, VmValue};

static ALL_OPS: OnceLock<Vec<&'static NativeOpEntry>> = OnceLock::new();

pub fn all_native_ops() -> &'static [&'static NativeOpEntry] {
    ALL_OPS.get_or_init(|| {
        crate::modules::NATIVE_MODULES
            .iter()
            .flat_map(|entries| entries.iter().copied())
            .collect()
    })
}

fn op_key(module: &str, namespace: &str, symbol: &str) -> String {
    format!("{module}\u{1}{namespace}\u{1}{symbol}")
}

struct Registry {
    by_id: Vec<Option<&'static NativeOpEntry>>,
    by_name: FxHashMap<String, u64>,
    by_fn: FxHashMap<usize, &'static NativeOpEntry>,
    modules: BTreeSet<&'static str>,
}

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut reg = Registry {
            by_id: vec![None; FIRST_REGISTRY_ID as usize],
            by_name: FxHashMap::default(),
            by_fn: FxHashMap::default(),
            modules: BTreeSet::new(),
        };
        for &entry in all_native_ops() {
            let key = op_key(
                entry.module_id(),
                entry.namespace_path(),
                entry.symbol_name(),
            );
            if reg.by_name.contains_key(&key) {
                continue;
            }
            let known = (entry.module_id() == CORE_MODULE)
                .then(|| KnownOp::from_names(entry.namespace_path(), entry.symbol_name()))
                .flatten();
            let id = match known {
                Some(op) => op.id(),
                None => reg.by_id.len() as u64,
            };
            if id as usize == reg.by_id.len() {
                reg.by_id.push(Some(entry));
            } else {
                reg.by_id[id as usize] = Some(entry);
            }
            reg.by_name.insert(key, id);
            reg.by_fn.entry(entry.func() as usize).or_insert(entry);
            reg.modules.insert(entry.module_id());
        }
        reg
    })
}

pub fn native_op_id(module: &str, namespace: &str, symbol: &str) -> Option<u64> {
    registry()
        .by_name
        .get(&op_key(module, namespace, symbol))
        .copied()
}

pub fn find_native_op_entry(op_id: u64) -> Option<&'static NativeOpEntry> {
    registry()
        .by_id
        .get(usize::try_from(op_id).ok()?)
        .copied()
        .flatten()
}

pub fn native_op_fn(id: u64) -> Option<varn_types::NativeFn> {
    find_native_op_entry(id).map(NativeOpEntry::func)
}

pub fn native_op_name_by_fn(f: varn_types::NativeFn) -> Option<&'static str> {
    registry().by_fn.get(&(f as usize)).map(|e| e.symbol_name())
}

pub fn check_native_fn_capability(
    f: varn_types::NativeFn,
    ctx: &dyn NativeCtx,
) -> Result<(), String> {
    let Some(entry) = registry().by_fn.get(&(f as usize)) else {
        return Ok(());
    };
    match entry.capability() {
        Some(capability) if !ctx.has_capability(capability) => Err(format!(
            "E_HOST_PERMISSION_DENIED:id={}:{}:capability={capability}",
            entry.module_id(),
            entry.symbol_name()
        )),
        Some(_) | None => Ok(()),
    }
}

pub fn all_native_module_ids() -> impl Iterator<Item = &'static str> {
    registry().modules.iter().copied()
}

pub fn has_native_module_id(id: &str) -> bool {
    registry().modules.contains(id)
}

mod devnull;
mod modules;

pub use devnull::DevNullModuleCtx;
pub(crate) use modules::build_module;
pub use modules::*;
