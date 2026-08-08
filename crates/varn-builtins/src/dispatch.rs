pub(crate) mod entry;

pub use entry::DispatchEntry;
use rustc_hash::FxHashMap;
use std::rc::Rc;
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
    unsafe {
        let start = &__VARN_OPS_START_MARKER as *const NativeOpEntry;
        let end = &__VARN_OPS_END_MARKER as *const NativeOpEntry;
        let len = (end as usize - start as usize) / std::mem::size_of::<NativeOpEntry>();
        let slice = std::slice::from_raw_parts(start, len);
        slice.iter().filter(|e| !e.func_ptr.is_null())
    }

    #[cfg(not(target_os = "windows"))]
    unsafe {
        let start = &__VARN_OPS_START as *const NativeOpEntry;
        let end = &__VARN_OPS_END as *const NativeOpEntry;
        let len = (end as usize - start as usize) / std::mem::size_of::<NativeOpEntry>();
        let slice = std::slice::from_raw_parts(start, len);
        slice.iter().filter(|e| !e.func_ptr.is_null())
    }
}

static FALLBACK_ENTRIES: OnceLock<std::sync::Mutex<Vec<&'static [&'static NativeOpEntry]>>> =
    OnceLock::new();

pub fn register_fallback_module_entries(entries: &'static [&'static NativeOpEntry]) {
    let mutex = FALLBACK_ENTRIES.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    if let Ok(mut guard) = mutex.lock() {
        guard.push(entries);
    }
}

pub fn all_native_ops() -> Vec<&'static NativeOpEntry> {
    let mut list: Vec<&'static NativeOpEntry> = iter_native_ops().collect();
    if let Some(mutex) = FALLBACK_ENTRIES.get() {
        if let Ok(guard) = mutex.lock() {
            for slice in guard.iter() {
                for &entry in *slice {
                    if !entry.func_ptr.is_null() && !list.iter().any(|e| std::ptr::eq(*e, entry)) {
                        list.push(entry);
                    }
                }
            }
        }
    }
    list
}

static TABLE: OnceLock<FxHashMap<u64, DispatchEntry>> = OnceLock::new();

fn build_table() -> FxHashMap<u64, DispatchEntry> {
    let mut table = FxHashMap::with_capacity_and_hasher(512, Default::default());
    for entry in all_native_ops() {
        let module = entry.module_id();
        let symbol = entry.symbol_name();
        let ns = entry.namespace_path();
        let id = if ns.is_empty() {
            entry::compound_op_id(module, symbol)
        } else {
            entry::compound_op_id3(module, ns, symbol)
        };
        table.insert(
            id,
            DispatchEntry {
                id,
                module_id: module,
                name: symbol,
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
    for entry in all_native_ops() {
        map.entry(entry.module_id().to_string())
            .or_insert_with(Vec::new)
            .push(entry);
    }
    map
}

pub fn find_native_op_entry(op_id: u64) -> Option<&'static NativeOpEntry> {
    for entry in all_native_ops() {
        let module = entry.module_id();
        let symbol = entry.symbol_name();
        let ns = entry.namespace_path();
        let id = if ns.is_empty() {
            entry::compound_op_id(module, symbol)
        } else {
            entry::compound_op_id3(module, ns, symbol)
        };
        if id == op_id {
            return Some(entry);
        }
    }
    None
}

pub fn describe_op(id: u64) -> Option<OpMeta> {
    for entry in all_native_ops() {
        let module = entry.module_id();
        let symbol = entry.symbol_name();
        let ns = entry.namespace_path();
        let op_id = if ns.is_empty() {
            entry::compound_op_id(module, symbol)
        } else {
            entry::compound_op_id3(module, ns, symbol)
        };
        if op_id == id {
            return Some(OpMeta {
                name: symbol,
                op_id,
                is_async: false,
                capability: None,
            });
        }
    }
    None
}

/// Resolve a stable op-id to its native function pointer, for callers that want
/// to invoke it through their own native-call path (preserving their error and
/// profiling semantics) rather than the wrapped [`dispatch_runtime_op`].
pub fn native_op_fn(id: u64) -> Option<varn_types::NativeFn> {
    let table = TABLE.get_or_init(build_table);
    table.get(&id).map(|e| e.func)
}

pub fn native_fast_op_fn(id: u64) -> Option<(*const u8, varn_types::SignatureDescriptor)> {
    let entry = find_native_op_entry(id)?;
    if !entry.raw_func_ptr.is_null() {
        Some((entry.raw_func_ptr, entry.signature))
    } else {
        None
    }
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

fn resolve_ns<'a>(
    root: VmValue,
    ns_path: &str,
    ctx: &mut dyn NativeCtx,
    cache: &'a mut FxHashMap<String, VmValue>,
) -> VmValue {
    let parts: Vec<&str> = if ns_path.contains(':') {
        ns_path.split(':').filter(|p| !p.is_empty()).collect()
    } else {
        ns_path.split('.').filter(|p| !p.is_empty()).collect()
    };

    let mut path_key = String::new();
    let mut current = root;

    for part in parts {
        let child_key = if path_key.is_empty() {
            part.to_string()
        } else {
            format!("{}.{}", path_key, part)
        };

        if let Some(&existing) = cache.get(&child_key) {
            current = existing;
        } else {
            let child = ctx.get_field(current, part).unwrap_or_else(|| {
                let new_obj = ctx.alloc_object();
                ctx.set_field(current, part, new_obj);
                new_obj
            });
            cache.insert(child_key.clone(), child);
            current = child;
        }
        path_key = child_key;
    }
    current
}

pub(crate) fn build_module(id: &str, ctx: &mut dyn NativeCtx) -> Option<VmValue> {
    let all_ops = all_native_ops();
    let entries: Vec<&'static NativeOpEntry> = all_ops
        .into_iter()
        .filter(|e| e.module_id() == id)
        .collect();
    if entries.is_empty() {
        return None;
    }

    let root = ctx.alloc_object();
    let mut ns_cache = FxHashMap::default();

    for entry in entries {
        let symbol = entry.symbol_name();
        let ns_path = entry.namespace_path();

        let target = if ns_path.is_empty() {
            root
        } else {
            resolve_ns(root, ns_path, ctx, &mut ns_cache)
        };

        let val = match entry.entry_kind {
            0x09 => ctx.call_static(entry.func()),
            0x10 => (entry.func())(ctx, &[]).unwrap_or(VmValue::null()),
            // Class-qualified members (instance/static method, getter, setter)
            // belong to a class built by its ClassDef (0x10), not to the module
            // object. They exist only to be op-id-addressable for direct dispatch.
            0x03 | 0x04 | 0x05 | 0x06 | 0x11 | 0x12 | 0x13 | 0x14 | 0x15 => continue,
            _ => ctx.alloc_fn(entry.func(), symbol),
        };

        ctx.set_field(target, symbol, val);
    }

    Some(ctx.finalize(root))
}

pub fn register_globals_vm(ctx: &mut dyn NativeCtx) -> rustc_hash::FxHashMap<Rc<str>, VmValue> {
    let mut out = rustc_hash::FxHashMap::default();
    out.insert(Rc::from("isIsolate"), VmValue::from_bool(false));

    if let Some(globals_nv) = build_module("globals", ctx) {
        collect_module_fields("globals", globals_nv, ctx, &mut out);
    }
    if let Some(core_nv) = build_module("core", ctx) {
        out.insert(Rc::from("core"), core_nv);
    }
    out
}

fn collect_module_fields(
    module_id: &str,
    module_nv: VmValue,
    ctx: &dyn NativeCtx,
    out: &mut rustc_hash::FxHashMap<Rc<str>, VmValue>,
) {
    for entry in all_native_ops() {
        if entry.module_id() != module_id {
            continue;
        }
        if !entry.namespace_path().is_empty() {
            continue;
        }
        let symbol = entry.symbol_name();
        if let Some(v) = ctx.get_field(module_nv, symbol) {
            out.insert(Rc::from(symbol), v);
        }
    }
}

pub fn has_native_module_id(id: &str) -> bool {
    let map = MODULE_OPS.get_or_init(build_module_ops_index);
    map.contains_key(id)
}

pub fn all_native_module_ids() -> Vec<String> {
    let map = MODULE_OPS.get_or_init(build_module_ops_index);
    map.keys().cloned().collect()
}

pub struct DevNullModuleCtx;

impl varn_types::NativeCtx for DevNullModuleCtx {
    fn alloc_str(&mut self, _s: &str) -> VmValue {
        VmValue::null()
    }
    fn alloc_str_owned(&mut self, _s: String) -> VmValue {
        VmValue::null()
    }
    fn alloc_array(&mut self, _items: Vec<VmValue>) -> VmValue {
        VmValue::null()
    }
    fn alloc_object(&mut self) -> VmValue {
        VmValue::null()
    }
    fn alloc_range(&mut self, _s: i64, _e: i64, _i: bool) -> VmValue {
        VmValue::null()
    }
    fn alloc_fn(&mut self, _f: varn_types::NativeFn, _name: &'static str) -> VmValue {
        VmValue::null()
    }
    fn alloc_class(&mut self, _c: std::rc::Rc<varn_types::ClassObj>) -> VmValue {
        VmValue::null()
    }
    fn is_string(&self, _v: VmValue) -> bool {
        false
    }
    fn is_array(&self, _v: VmValue) -> bool {
        false
    }
    fn str_repr(&self, _v: VmValue) -> String {
        String::new()
    }
    fn str_owned(&self, _v: VmValue) -> Option<String> {
        None
    }
    fn array_len(&self, _arr: VmValue) -> usize {
        0
    }
    fn array_get(&self, _arr: VmValue, _idx: usize) -> Option<VmValue> {
        None
    }
    fn array_set(&mut self, _arr: VmValue, _idx: usize, _val: VmValue) {}
    fn array_push(&mut self, _arr: VmValue, _val: VmValue) {}
    fn array_pop(&mut self, _arr: VmValue) -> Option<VmValue> {
        None
    }
    fn array_for_each(&self, _arr: VmValue, _f: &mut dyn FnMut(VmValue, usize)) {}
    fn get_field(&self, _obj: VmValue, _key: &str) -> Option<VmValue> {
        None
    }
    fn set_field(&mut self, _obj: VmValue, _key: &str, _val: VmValue) {}
    fn call_vm(&mut self, _c: VmValue, _a: &[VmValue]) -> Result<VmValue, String> {
        Ok(VmValue::null())
    }
    fn spawn_vm(&mut self, _c: VmValue, _a: &[VmValue]) -> Result<VmValue, String> {
        Ok(VmValue::null())
    }
    fn set_timer(
        &mut self,
        _ms: u64,
        _r: bool,
        _c: VmValue,
        _a: &[VmValue],
    ) -> Result<usize, String> {
        Ok(0)
    }
    fn clear_timer(&mut self, _id: usize) -> Result<(), String> {
        Ok(())
    }
    fn suspend_timer(&mut self, _ms: u64) -> VmValue {
        VmValue::null()
    }
    fn resources(&mut self) -> &mut varn_types::ResourceStore {
        panic!("DevNullModuleCtx::resources")
    }
    fn extract(&self, _v: VmValue) -> varn_types::Value {
        varn_types::Value::Null
    }
    fn intern(&mut self, _v: varn_types::Value) -> VmValue {
        VmValue::null()
    }
    fn call_static(&mut self, _f: varn_types::NativeFn) -> VmValue {
        VmValue::null()
    }
}
