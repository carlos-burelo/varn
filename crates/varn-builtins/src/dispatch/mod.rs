pub(crate) mod entry;

pub use entry::DispatchEntry;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use varn_core::op_meta::OpMeta;
use varn_types::{NativeCtx, NativeOpEntry, VmValue};

unsafe extern "C" {
    #[cfg(target_os = "macos")]
    #[link_name = "\x01section$start$__DATA$varn_ops"]
    static __VARN_OPS_START: NativeOpEntry;
    #[cfg(target_os = "macos")]
    #[link_name = "\x01section$end$__DATA$varn_ops"]
    static __VARN_OPS_END: NativeOpEntry;

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[link_name = "__start_varn_ops"]
    static __VARN_OPS_START: NativeOpEntry;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[link_name = "__stop_varn_ops"]
    static __VARN_OPS_END: NativeOpEntry;
}

#[cfg(target_os = "windows")]
#[used]
#[unsafe(link_section = ".varn_ops$A")]
pub static __VARN_OPS_START_MARKER: NativeOpEntry = unsafe { std::mem::zeroed() };

#[cfg(target_os = "windows")]
#[used]
#[unsafe(link_section = ".varn_ops$C")]
pub static __VARN_OPS_END_MARKER: NativeOpEntry = unsafe { std::mem::zeroed() };

pub fn iter_native_ops() -> impl Iterator<Item = &'static NativeOpEntry> {
    #[cfg(target_os = "windows")]
    let slice: &'static [NativeOpEntry] = unsafe {
        let start = &__VARN_OPS_START_MARKER as *const NativeOpEntry;
        let end = &__VARN_OPS_END_MARKER as *const NativeOpEntry;
        let start_u = start as usize;
        let end_u = end as usize;
        if end_u <= start_u {
            &[]
        } else {
            let len = (end_u - start_u) / std::mem::size_of::<NativeOpEntry>();
            std::slice::from_raw_parts(start, len)
        }
    };

    #[cfg(not(target_os = "windows"))]
    let slice: &'static [NativeOpEntry] = unsafe {
        let start = &__VARN_OPS_START as *const NativeOpEntry;
        let end = &__VARN_OPS_END as *const NativeOpEntry;
        let len = (end as usize - start as usize) / std::mem::size_of::<NativeOpEntry>();
        std::slice::from_raw_parts(start, len)
    };

    slice.iter().filter(|e| !e.func_ptr.is_null())
}

static FALLBACK_ENTRIES: OnceLock<std::sync::Mutex<Vec<&'static [&'static NativeOpEntry]>>> =
    OnceLock::new();

pub fn register_fallback_module_entries(entries: &'static [&'static NativeOpEntry]) {
    let mutex = FALLBACK_ENTRIES.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    if let Ok(mut guard) = mutex.lock() {
        guard.push(entries);
    }
}

static ALL_OPS: OnceLock<Vec<&'static NativeOpEntry>> = OnceLock::new();

pub fn all_native_ops() -> &'static [&'static NativeOpEntry] {
    ALL_OPS.get_or_init(|| {
        let mut list: Vec<&'static NativeOpEntry> = Vec::new();
        let mut seen: rustc_hash::FxHashSet<usize> = rustc_hash::FxHashSet::default();
        let mut push = |entry: &'static NativeOpEntry, list: &mut Vec<_>| {
            if !entry.func_ptr.is_null() && seen.insert(entry as *const NativeOpEntry as usize) {
                list.push(entry);
            }
        };
        for entry in iter_native_ops() {
            push(entry, &mut list);
        }
        if let Some(mutex) = FALLBACK_ENTRIES.get() {
            if let Ok(guard) = mutex.lock() {
                for slice in guard.iter() {
                    for &entry in *slice {
                        push(entry, &mut list);
                    }
                }
            }
        }
        list
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

pub fn capability_for_op(module_id: &str, namespace: &str, symbol: &str) -> Option<&'static str> {
    let _ = namespace;
    let sym = symbol.trim_end_matches('$');
    match (module_id, sym) {
        ("runtime:ffi", _) => Some("sys.ffi"),
        ("runtime:sys", "env" | "setEnv") => Some("sys.env"),
        ("runtime:process", "exec" | "spawnProcess") => Some("sys.exec"),
        _ => None,
    }
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
                capability: capability_for_op(
                    entry.module_id(),
                    entry.namespace_path(),
                    entry.symbol_name(),
                ),
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
        capability: capability_for_op(
            entry.module_id(),
            entry.namespace_path(),
            entry.symbol_name(),
        ),
    })
}

pub fn check_native_op_capability(id: u64, ctx: &dyn NativeCtx) -> Result<(), String> {
    let table = TABLE.get_or_init(build_table);
    match table.get(&id) {
        None => Ok(()),
        Some(entry) => match entry.capability {
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
        },
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
        if let Some(capability) = entry.capability {
            if !ctx.has_capability(capability) {
                return Err(format!(
                    "E_RUNTIME_PERMISSION_DENIED:id={id}:capability={capability}"
                ));
            }
        }
        return (entry.func)(ctx, args).map_err(|err| format!("E_RUNTIME_FAILURE:id={id}:{err}"));
    }
    Err(format!("E_RUNTIME_UNKNOWN_WIRE:id={id}"))
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    fn symbols_of(module: &str) -> std::collections::BTreeSet<String> {
        all_native_ops()
            .iter()
            .filter(|e| e.module_id() == module)
            .map(|e| e.symbol_name().trim_end_matches('$').to_string())
            .collect()
    }

    #[test]
    fn mapped_symbols_exist() {
        for (module, symbols) in [
            ("runtime:ffi", &["dlopen", "dlsym", "call"][..]),
            ("runtime:sys", &["env", "setEnv"][..]),
            ("runtime:process", &["exec", "spawnProcess"][..]),
        ] {
            let have = symbols_of(module);
            for &s in symbols {
                assert!(
                    have.iter().any(|h| h == s),
                    "capability mapping references missing op {module}:{s}"
                );
                assert!(
                    capability_for_op(module, "", s).is_some(),
                    "mapped op {module}:{s} must resolve a capability"
                );
            }
        }
    }

    #[test]
    fn ffi_ops_require_sys_ffi() {
        for e in all_native_ops() {
            if e.module_id() == "runtime:ffi" {
                assert_eq!(
                    capability_for_op(e.module_id(), e.namespace_path(), e.symbol_name()),
                    Some("sys.ffi"),
                    "every ffi op must gate sys.ffi"
                );
            }
        }
    }
}

mod devnull;
mod modules;

pub use devnull::DevNullModuleCtx;
pub(crate) use modules::build_module;
pub use modules::*;
