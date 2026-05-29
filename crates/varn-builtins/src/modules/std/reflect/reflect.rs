use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

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

#[varn_module("runtime:reflect")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn("reflectDefineMetadata")]
    pub fn define_metadata(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let key = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let val = args.get(1).cloned().unwrap_or(VmValue::null());
        let target = args.get(2).map(|&v| target_key(ctx, v)).unwrap_or_default();
        with_metadata(|m| {
            m.entry(target).or_default().insert(key, val);
        });
        Ok(VmValue::null())
    }

    #[varn_fn("reflectGetMetadata")]
    pub fn get_metadata(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let key = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let target = args.get(1).map(|&v| target_key(ctx, v)).unwrap_or_default();
        let res = with_metadata(|m| {
            m.get(&target)
                .and_then(|m| m.get(&key))
                .cloned()
                .unwrap_or(VmValue::null())
        });
        Ok(res)
    }

    #[varn_fn("reflectHasMetadata")]
    pub fn has_metadata(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let key = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let target = args.get(1).map(|&v| target_key(ctx, v)).unwrap_or_default();
        let found = with_metadata(|m| {
            m.get(&target)
                .map(|m| m.contains_key(&key))
                .unwrap_or(false)
        });
        Ok(VmValue::from_bool(found))
    }

    #[varn_fn("reflectCreateMetaKey")]
    pub fn create_meta_key(ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        let id = META_KEY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let key_name = format!("meta_{}", id);
        Ok(ctx.alloc_str_owned(key_name))
    }

    #[varn_fn("reflectSetMetaKey")]
    pub fn set_meta_key(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        if let (Some(&meta_id_val), Some(&target), Some(&val)) =
            (args.first(), args.get(1), args.get(2))
        {
            let meta_id = ctx.str_repr(meta_id_val);
            let target_k = target_key(ctx, target);
            with_metadata(|m| {
                m.entry(target_k).or_default().insert(meta_id, val);
            });
        }
        Ok(VmValue::null())
    }

    #[varn_fn("reflectGetMetaKey")]
    pub fn get_meta_key(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        if let (Some(&meta_id_val), Some(&target)) = (args.first(), args.get(1)) {
            let meta_id = ctx.str_repr(meta_id_val);
            let target_k = target_key(ctx, target);
            let res = with_metadata(|m| {
                m.get(&target_k)
                    .and_then(|m| m.get(&meta_id))
                    .cloned()
                    .unwrap_or(VmValue::null())
            });
            return Ok(res);
        }
        Ok(VmValue::null())
    }

    #[varn_fn("reflectHasMetaKey")]
    pub fn has_meta_key(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        if let (Some(&meta_id_val), Some(&target)) = (args.first(), args.get(1)) {
            let meta_id = ctx.str_repr(meta_id_val);
            let target_k = target_key(ctx, target);
            let found = with_metadata(|m| {
                m.get(&target_k)
                    .map(|m| m.contains_key(&meta_id))
                    .unwrap_or(false)
            });
            return Ok(VmValue::from_bool(found));
        }
        Ok(VmValue::from_bool(false))
    }
}
