pub(crate) mod entry;

pub use entry::DispatchEntry;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use varn_core::op_meta::OpMeta;
use varn_types::{NativeCtx, NativeOpEntry, VmValue};

extern "C" {
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
#[link_section = ".varn_ops$A"]
pub static __VARN_OPS_START_MARKER: NativeOpEntry = unsafe { std::mem::zeroed() };

#[cfg(target_os = "windows")]
#[used]
#[link_section = ".varn_ops$C"]
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

/// Every registered native op: the linker-section walk unioned with the
/// fallback marker arrays, deduplicated by address.
///
/// Both sources point at the *same* statics — `varn_contract!` emits one
/// `NativeOpEntry` per symbol and lists it in the module's
/// `__VARN_LINK_MARKER_*` array — so `ptr::eq` collapses them and neither
/// source is more authoritative than the other. The fallback exists so the
/// table stays complete when the linker spreads `.varn_ops` across codegen
/// units.
///
/// Frozen on first call, like [`TABLE`] and [`MODULE_OPS`]. Every registration
/// happens in `register_provider()` (via `force_link_builtins`), which every
/// entry point calls before touching dispatch.
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

/// `op_id` of an entry, as the compound hash the compiler emits for it.
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
                capability: None,
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

/// Reverse index from a native function pointer back to its symbol name.
///
/// Call sites that reach a native through a resolved pointer — the JIT's
/// `CallNativeOp` lowering bakes the address at compile time and keeps no
/// op-id — have no name to report to the profiler. Without this they bump the
/// call counter and vanish from the per-name table, which is how `bench -v`
/// came to show 77k native calls against ~1.1k attributed ones.
///
/// Built once, only ever consulted from profiling paths.
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
        capability: None,
    })
}

/// Resolve a stable op-id to its native function pointer, for callers that want
/// to invoke it through their own native-call path (preserving their error and
/// profiling semantics) rather than the wrapped [`dispatch_runtime_op`].
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

mod devnull;
mod modules;

pub use devnull::DevNullModuleCtx;
pub(crate) use modules::build_module;
pub use modules::*;
