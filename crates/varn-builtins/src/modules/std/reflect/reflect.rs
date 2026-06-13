use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

/// Native implementation backing the `runtime:reflect` contract
/// (`src/modules/std/reflect/runtime/reflect_runtime.vn`).
pub struct ReflectRuntime;

thread_local! {
    static METADATA: RefCell<HashMap<String, HashMap<String, VmValue>>> =
        RefCell::new(HashMap::new());
}

fn with_metadata<R, F: FnOnce(&mut HashMap<String, HashMap<String, VmValue>>) -> R>(f: F) -> R {
    METADATA.with(|m| f(&mut m.borrow_mut()))
}

fn target_key(ctx: &dyn NativeCtx, v: VmValue) -> String {
    if v.is_heap() {
        format!("heap:{:x}", v.as_heap_idx())
    } else if v.is_int() {
        format!("int:{}", v.as_int())
    } else {
        ctx.str_repr(v)
    }
}

static META_KEY_COUNTER: AtomicU64 = AtomicU64::new(1);

varn_contract! {
    module: "runtime:reflect",
    contract: "src/modules/std/reflect/runtime/reflect_runtime.vn",
    impl ReflectRuntime {
        fn reflectDefineMetadata(ctx: &mut dyn NativeCtx, metadataKey: VmValue, metadataValue: VmValue, target: VmValue) -> Result<(), String> {
            let key = ctx.str_repr(metadataKey);
            let target_k = target_key(ctx, target);
            with_metadata(|m| {
                m.entry(target_k).or_default().insert(key, metadataValue);
            });
            Ok(())
        }
        fn reflectGetMetadata(ctx: &mut dyn NativeCtx, metadataKey: VmValue, target: VmValue) -> Result<VmValue, String> {
            let key = ctx.str_repr(metadataKey);
            let target_k = target_key(ctx, target);
            Ok(with_metadata(|m| {
                m.get(&target_k).and_then(|m| m.get(&key)).cloned().unwrap_or(VmValue::null())
            }))
        }
        fn reflectHasMetadata(ctx: &mut dyn NativeCtx, metadataKey: VmValue, target: VmValue) -> Result<bool, String> {
            let key = ctx.str_repr(metadataKey);
            let target_k = target_key(ctx, target);
            Ok(with_metadata(|m| m.get(&target_k).map(|m| m.contains_key(&key)).unwrap_or(false)))
        }
        fn reflectCreateMetaKey(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            let id = META_KEY_COUNTER.fetch_add(1, Ordering::Relaxed);
            Ok(format!("meta_{id}"))
        }
        fn reflectSetMetaKey(ctx: &mut dyn NativeCtx, metaId: &str, target: VmValue, value: VmValue) -> Result<(), String> {
            let target_k = target_key(ctx, target);
            with_metadata(|m| {
                m.entry(target_k).or_default().insert(metaId.to_string(), value);
            });
            Ok(())
        }
        fn reflectGetMetaKey(ctx: &mut dyn NativeCtx, metaId: &str, target: VmValue) -> Result<VmValue, String> {
            let target_k = target_key(ctx, target);
            Ok(with_metadata(|m| {
                m.get(&target_k).and_then(|m| m.get(metaId)).cloned().unwrap_or(VmValue::null())
            }))
        }
        fn reflectHasMetaKey(ctx: &mut dyn NativeCtx, metaId: &str, target: VmValue) -> Result<bool, String> {
            let target_k = target_key(ctx, target);
            Ok(with_metadata(|m| m.get(&target_k).map(|m| m.contains_key(metaId)).unwrap_or(false)))
        }
    }
}
