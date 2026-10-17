pub(crate) mod entry;

pub use entry::DispatchEntry;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use varn_core::op_meta::OpMeta;
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

fn entry_op_id(entry: &NativeOpEntry) -> u64 {
    let module = entry.module_id();
    let symbol = entry.symbol_name();
    let ns = entry.namespace_path();
    if ns.is_empty() {
        entry::compound_op_id(module, symbol)
    } else {
        entry::compound_op_id3(module, ns, symbol)
    }
}

static ENTRY_BY_OP_ID: OnceLock<FxHashMap<u64, &'static NativeOpEntry>> = OnceLock::new();

fn build_entry_index() -> FxHashMap<u64, &'static NativeOpEntry> {
    let mut map = FxHashMap::with_capacity_and_hasher(512, Default::default());
    for &entry in all_native_ops() {
        map.entry(entry_op_id(entry)).or_insert(entry);
    }
    map
}

static TABLE: OnceLock<FxHashMap<u64, DispatchEntry>> = OnceLock::new();
static FN_TABLE: OnceLock<FxHashMap<usize, DispatchEntry>> = OnceLock::new();

fn fn_table() -> &'static FxHashMap<usize, DispatchEntry> {
    FN_TABLE.get_or_init(|| {
        let mut table = FxHashMap::default();
        for &entry in all_native_ops() {
            table.entry(entry.func() as usize).or_insert(DispatchEntry {
                id: entry_op_id(entry),
                module_id: entry.module_id(),
                name: entry.symbol_name(),
                func: entry.func(),
                capability: entry.capability(),
            });
        }
        table
    })
}

fn build_table() -> FxHashMap<u64, DispatchEntry> {
    let mut table = FxHashMap::with_capacity_and_hasher(512, Default::default());
    for &entry in all_native_ops() {
        let id = entry_op_id(entry);
        table.insert(
            id,
            DispatchEntry {
                id,
                module_id: entry.module_id(),
                name: entry.symbol_name(),
                func: entry.func(),
                capability: entry.capability(),
            },
        );
    }
    table
}

static MODULE_OPS: OnceLock<FxHashMap<String, Vec<&'static NativeOpEntry>>> = OnceLock::new();

fn build_module_ops_index() -> FxHashMap<String, Vec<&'static NativeOpEntry>> {
    let mut map = FxHashMap::default();
    for &entry in all_native_ops() {
        map.entry(entry.module_id().to_string())
            .or_insert_with(Vec::new)
            .push(entry);
    }
    map
}

pub fn find_native_op_entry(op_id: u64) -> Option<&'static NativeOpEntry> {
    ENTRY_BY_OP_ID
        .get_or_init(build_entry_index)
        .get(&op_id)
        .copied()
}

static NAME_BY_FN: OnceLock<FxHashMap<usize, &'static str>> = OnceLock::new();

pub fn native_op_name_by_fn(f: varn_types::NativeFn) -> Option<&'static str> {
    NAME_BY_FN
        .get_or_init(|| {
            let mut m = FxHashMap::with_capacity_and_hasher(512, Default::default());
            for &entry in all_native_ops() {
                m.entry(entry.func() as usize)
                    .or_insert(entry.symbol_name());
            }
            m
        })
        .get(&(f as usize))
        .copied()
}

pub fn describe_op(id: u64) -> Option<OpMeta> {
    find_native_op_entry(id).map(|entry| OpMeta {
        name: entry.symbol_name(),
        op_id: id,
        is_async: false,
        capability: entry.capability(),
    })
}

pub fn check_native_op_capability(id: u64, ctx: &dyn NativeCtx) -> Result<(), String> {
    let table = TABLE.get_or_init(build_table);
    match table.get(&id) {
        None => Ok(()),
        Some(entry) => check_entry(entry, ctx),
    }
}

pub fn check_native_fn_capability(
    f: varn_types::NativeFn,
    ctx: &dyn NativeCtx,
) -> Result<(), String> {
    match fn_table().get(&(f as usize)) {
        None => Ok(()),
        Some(entry) => check_entry(entry, ctx),
    }
}

fn check_entry(entry: &DispatchEntry, ctx: &dyn NativeCtx) -> Result<(), String> {
    match entry.capability {
        None => Ok(()),
        Some(capability) => {
            if ctx.has_capability(capability) {
                Ok(())
            } else {
                Err(format!(
                    "E_HOST_PERMISSION_DENIED:id={}:{}:capability={capability}",
                    entry.module_id, entry.name
                ))
            }
        }
    }
}

pub fn native_op_fn(id: u64) -> Option<varn_types::NativeFn> {
    let table = TABLE.get_or_init(build_table);
    table.get(&id).map(|e| e.func)
}

pub fn dispatch_runtime_op(
    id: u64,
    ctx: &mut dyn NativeCtx,
    args: &[VmValue],
) -> Result<VmValue, String> {
    let table = TABLE.get_or_init(build_table);
    if let Some(entry) = table.get(&id) {
        if let Some(capability) = entry.capability
            && !ctx.has_capability(capability)
        {
            return Err(format!(
                "E_RUNTIME_PERMISSION_DENIED:id={id}:capability={capability}"
            ));
        }
        return (entry.func)(ctx, args).map_err(|err| format!("E_RUNTIME_FAILURE:id={id}:{err}"));
    }
    Err(format!("E_RUNTIME_UNKNOWN_WIRE:id={id}"))
}

mod devnull;
mod modules;

pub use devnull::DevNullModuleCtx;
pub(crate) use modules::build_module;
pub use modules::*;
