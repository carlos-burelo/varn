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

// Map keys hash by content; routing them through `ctx.alloc_str` would
// intern every dynamic key (retaining it forever) and pay an extra heap
// allocation per lookup.
fn str_key(key: &str) -> Value {
    Value::Str(std::rc::Rc::from(key))
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
            let key_val = str_key(key);
            let found = m.borrow().get(&key_val).cloned();
            found.map(|v| ctx.intern(v))
        }
        fn set(ctx: &mut dyn NativeCtx, this: VmValue, key: &str, value: VmValue) {
            if let Some(m) = get_map(ctx, this) {
                let key_val = str_key(key);
                let val = ctx.extract(value);
                m.borrow_mut().insert(key_val, val);
            }
        }
        fn has(ctx: &mut dyn NativeCtx, this: VmValue, key: &str) -> bool {
            match get_map(ctx, this) {
                Some(m) => {
                    let key_val = str_key(key);
                    m.borrow().contains_key(&key_val)
                }
                None => false,
            }
        }
        fn delete(ctx: &mut dyn NativeCtx, this: VmValue, key: &str) -> bool {
            match get_map(ctx, this) {
                Some(m) => {
                    let key_val = str_key(key);
                    m.borrow_mut().remove(&key_val).is_some()
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
                Some(m) => {
                    let ks: Vec<Value> = m.borrow().keys().cloned().collect();
                    ks.into_iter().map(|k| ctx.intern(k)).collect()
                }
                None => Vec::new(),
            }
        }
        fn values(ctx: &mut dyn NativeCtx, this: VmValue) -> Vec<VmValue> {
            match get_map(ctx, this) {
                Some(m) => {
                    let vs: Vec<Value> = m.borrow().values().cloned().collect();
                    vs.into_iter().map(|v| ctx.intern(v)).collect()
                }
                None => Vec::new(),
            }
        }
        fn entries(ctx: &mut dyn NativeCtx, this: VmValue) -> Vec<VmValue> {
            match get_map(ctx, this) {
                Some(m) => {
                    let pairs: Vec<(Value, Value)> =
                        m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    pairs
                        .into_iter()
                        .map(|(k, v)| {
                            let kn = ctx.intern(k);
                            let vn = ctx.intern(v);
                            ctx.alloc_array(vec![kn, vn])
                        })
                        .collect()
                }
                None => Vec::new(),
            }
        }
        fn forEach(ctx: &mut dyn NativeCtx, this: VmValue, callback: VmValue) {
            if let Some(m) = get_map(ctx, this) {
                let pairs: Vec<(Value, Value)> =
                    m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                for (k, v) in pairs {
                    let kn = ctx.intern(k);
                    let vn = ctx.intern(v);
                    let _ = ctx.call_vm(callback, &[vn, kn, this]);
                }
            }
        }
        fn size(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            get_map(ctx, this).map(|m| m.borrow().len() as i64).unwrap_or(0)
        }
    }
}
