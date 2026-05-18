use std::collections::HashMap;
#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_getter, varn_method, varn_module};
use varn_types::value::MapRef;
use varn_types::{NativeCtx, Value, VmValue};

fn get_map(ctx: &dyn NativeCtx, this: VmValue) -> Option<MapRef> {
    if let Value::Map(m) = ctx.extract(this) {
        Some(m)
    } else {
        None
    }
}

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_class("Map")]
    pub mod map_class {
        use super::*;

        #[varn_method("constructor")]
        pub fn map_new(
            ctx: &mut dyn NativeCtx,
            _this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let map = MapRef::new(HashMap::new());
            Ok(ctx.intern(Value::Map(map)))
        }

        #[varn_method("set")]
        pub fn map_set(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let (Some(&key_nv), Some(&val_nv)) = (args.first(), args.get(1)) {
                if let Some(m) = get_map(ctx, this) {
                    let key = ctx.extract(key_nv);
                    let val = ctx.extract(val_nv);
                    m.borrow_mut().insert(key, val);
                    return Ok(this);
                }
            }
            Ok(VmValue::null())
        }

        #[varn_method("get")]
        pub fn map_get(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&key_nv) = args.first() {
                if let Some(m) = get_map(ctx, this) {
                    let key = ctx.extract(key_nv);
                    if let Some(val) = m.borrow().get(&key).cloned() {
                        return Ok(ctx.intern(val));
                    }
                }
            }
            Ok(VmValue::null())
        }

        #[varn_method("has")]
        pub fn map_has(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&key_nv) = args.first() {
                if let Some(m) = get_map(ctx, this) {
                    let key = ctx.extract(key_nv);
                    return Ok(VmValue::from_bool(m.borrow().contains_key(&key)));
                }
            }
            Ok(VmValue::from_bool(false))
        }

        #[varn_method("delete")]
        pub fn map_delete(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&key_nv) = args.first() {
                if let Some(m) = get_map(ctx, this) {
                    let key = ctx.extract(key_nv);
                    let removed = m.borrow_mut().remove(&key).is_some();
                    return Ok(VmValue::from_bool(removed));
                }
            }
            Ok(VmValue::from_bool(false))
        }

        #[varn_method("keys")]
        pub fn map_keys(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(m) = get_map(ctx, this) {
                let keys: Vec<VmValue> = m.borrow().keys().map(|k| ctx.intern(k.clone())).collect();
                return Ok(ctx.alloc_array(keys));
            }
            Ok(ctx.alloc_array(vec![]))
        }

        #[varn_method("values")]
        pub fn map_values(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(m) = get_map(ctx, this) {
                let vals: Vec<VmValue> =
                    m.borrow().values().map(|v| ctx.intern(v.clone())).collect();
                return Ok(ctx.alloc_array(vals));
            }
            Ok(ctx.alloc_array(vec![]))
        }

        #[varn_method("entries")]
        pub fn map_entries(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(m) = get_map(ctx, this) {
                let entries: Vec<VmValue> = m
                    .borrow()
                    .iter()
                    .map(|(k, v)| {
                        let k_nv = ctx.intern(k.clone());
                        let v_nv = ctx.intern(v.clone());
                        ctx.alloc_array(vec![k_nv, v_nv])
                    })
                    .collect();
                return Ok(ctx.alloc_array(entries));
            }
            Ok(ctx.alloc_array(vec![]))
        }

        #[varn_method("clear")]
        pub fn map_clear(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(m) = get_map(ctx, this) {
                m.borrow_mut().clear();
            }
            Ok(VmValue::null())
        }

        #[varn_method("forEach")]
        pub fn map_for_each(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&cb) = args.first() {
                if let Some(m) = get_map(ctx, this) {
                    let pairs: Vec<(Value, Value)> = m
                        .borrow()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    for (k, v) in pairs {
                        let k_nv = ctx.intern(k);
                        let v_nv = ctx.intern(v);
                        ctx.call_vm(cb, &[v_nv, k_nv, this])?;
                    }
                }
            }
            Ok(VmValue::null())
        }

        #[varn_getter("size")]
        pub fn map_size(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<VmValue, String> {
            if let Some(m) = get_map(ctx, this) {
                return Ok(VmValue::from_int(m.borrow().len() as i64));
            }
            Ok(VmValue::from_int(0))
        }
    }
}
