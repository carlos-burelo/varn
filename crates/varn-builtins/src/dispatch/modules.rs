use super::*;

fn resolve_ns(
    root: VmValue,
    ns_path: &str,
    ctx: &mut dyn NativeCtx,
    cache: &mut FxHashMap<String, VmValue>,
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
    let entries: Vec<&'static NativeOpEntry> = all_native_ops()
        .iter()
        .copied()
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

            0x03 | 0x04 | 0x05 | 0x06 | 0x11 | 0x12 | 0x13 | 0x14 | 0x15 => continue,
            _ => ctx.alloc_fn(entry.func(), symbol),
        };

        ctx.set_field(target, symbol, val);
    }

    Some(ctx.finalize(root))
}

const FLOAT_CONSTANTS: &[(&str, f64)] = &[("Infinity", f64::INFINITY), ("NaN", f64::NAN)];

pub fn register_globals_vm(ctx: &mut dyn NativeCtx) -> rustc_hash::FxHashMap<Arc<str>, VmValue> {
    let mut out = rustc_hash::FxHashMap::default();
    out.insert(Arc::from("isIsolate"), VmValue::from_bool(false));
    for (name, value) in FLOAT_CONSTANTS {
        out.insert(Arc::from(*name), VmValue::from_f64(*value));
    }

    if let Some(globals_nv) = build_module("globals", ctx) {
        collect_module_fields("globals", globals_nv, ctx, &mut out);
    }
    if let Some(core_nv) = build_module("core", ctx) {
        out.insert(Arc::from("core"), core_nv);
    }
    out
}

pub fn native_global_layout() -> &'static [&'static str] {
    static LAYOUT: OnceLock<Vec<&'static str>> = OnceLock::new();
    LAYOUT.get_or_init(|| {
        const SKIP_KINDS: &[u8] = &[0x03, 0x04, 0x05, 0x06, 0x11, 0x12, 0x13, 0x14, 0x15];

        let mut names: Vec<&'static str> = vec!["isIsolate"];
        names.extend(FLOAT_CONSTANTS.iter().map(|(name, _)| *name));
        let mut has_core = false;
        for e in all_native_ops() {
            if e.module_id() == "core" {
                has_core = true;
            }
            if e.module_id() == "globals"
                && e.namespace_path().is_empty()
                && !SKIP_KINDS.contains(&e.entry_kind)
            {
                names.push(e.symbol_name());
            }
        }
        if has_core {
            names.push("core");
        }
        names.sort_unstable();
        names.dedup();

        let mut out: Vec<&'static str> = Vec::with_capacity(names.len());
        for p in ["print"] {
            if let Some(pos) = names.iter().position(|n| *n == p) {
                out.push(names.remove(pos));
            }
        }
        out.extend(names);
        out
    })
}

pub fn native_global_index(name: &str) -> Option<u32> {
    native_global_layout()
        .iter()
        .position(|n| *n == name)
        .map(|i| i as u32)
}

fn collect_module_fields(
    module_id: &str,
    module_nv: VmValue,
    ctx: &dyn NativeCtx,
    out: &mut rustc_hash::FxHashMap<Arc<str>, VmValue>,
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
            out.insert(Arc::from(symbol), v);
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
