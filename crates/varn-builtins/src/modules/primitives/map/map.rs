use varn_op_macros::varn_contract;
use varn_types::value::MapRef;
use varn_types::{NativeCtx, Value, VmValue};

pub struct Map;

fn get_map(ctx: &dyn NativeCtx, this: VmValue) -> Option<MapRef> {
    if let Value::Map(m) = ctx.extract(this) {
        Some(m)
    } else {
        None
    }
}

varn_contract! {
    module: "globals",
    class: "Map",
    contract: "src/modules/primitives/map/map.vn",
    impl Map {
        fn constructor(ctx: &mut dyn NativeCtx, _this: VmValue) -> VmValue {
            ctx.intern(Value::Map(MapRef::new(varn_types::value::ValueMap::default())))
        }

        fn get(ctx: &mut dyn NativeCtx, this: VmValue, key: &str) -> Option<VmValue> {
            let m = get_map(ctx, this)?;
            let k = ctx.str_map_key(key);
            let found = m.borrow().get(&k).copied();
            found
        }
        fn set(ctx: &mut dyn NativeCtx, this: VmValue, key: &str, value: VmValue) {
            if let Some(m) = get_map(ctx, this) {
                let k = ctx.str_map_key(key);
                m.borrow_mut().insert(k, value);
                // Interior-mutability store: no opcode barrier sees it.
                ctx.collection_write_barrier(this, value);
            }
        }
        fn has(ctx: &mut dyn NativeCtx, this: VmValue, key: &str) -> bool {
            match get_map(ctx, this) {
                Some(m) => {
                    let k = ctx.str_map_key(key);
                    m.borrow().contains_key(&k)
                }
                None => false,
            }
        }
        fn delete(ctx: &mut dyn NativeCtx, this: VmValue, key: &str) -> bool {
            match get_map(ctx, this) {
                Some(m) => {
                    let k = ctx.str_map_key(key);
                    m.borrow_mut().remove(&k).is_some()
                }
                None => false,
            }
        }
        fn clear(ctx: &mut dyn NativeCtx, this: VmValue) {
            if let Some(m) = get_map(ctx, this) {
                m.borrow_mut().clear();
            }
        }
        fn keys(ctx: &mut dyn NativeCtx, this: VmValue) -> Vec<VmValue> {
            match get_map(ctx, this) {
                Some(m) => m.borrow().keys().map(|k| k.0).collect(),
                None => Vec::new(),
            }
        }
        fn values(ctx: &mut dyn NativeCtx, this: VmValue) -> Vec<VmValue> {
            match get_map(ctx, this) {
                Some(m) => m.borrow().values().copied().collect(),
                None => Vec::new(),
            }
        }
        fn entries(ctx: &mut dyn NativeCtx, this: VmValue) -> Vec<VmValue> {
            match get_map(ctx, this) {
                Some(m) => {
                    let pairs: Vec<(VmValue, VmValue)> =
                        m.borrow().iter().map(|(k, v)| (k.0, *v)).collect();
                    pairs
                        .into_iter()
                        .map(|(k, v)| ctx.alloc_array(vec![k, v]))
                        .collect()
                }
                None => Vec::new(),
            }
        }
        fn forEach(ctx: &mut dyn NativeCtx, this: VmValue, callback: VmValue) {
            if let Some(m) = get_map(ctx, this) {
                let pairs: Vec<(VmValue, VmValue)> =
                    m.borrow().iter().map(|(k, v)| (k.0, *v)).collect();
                for (k, v) in pairs {
                    let _ = ctx.call_vm(callback, &[v, k, this]);
                }
            }
        }
        fn size(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            get_map(ctx, this).map(|m| m.borrow().len() as i64).unwrap_or(0)
        }
    }
}
