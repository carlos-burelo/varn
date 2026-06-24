use std::collections::HashSet;
use varn_op_macros::varn_contract;
use varn_types::value::SetRef;
use varn_types::{NativeCtx, Value, VmValue};

pub struct Set;

fn get_set(ctx: &dyn NativeCtx, this: VmValue) -> Option<SetRef> {
    if let Value::Set(s) = ctx.extract(this) {
        Some(s)
    } else {
        None
    }
}

varn_contract! {
    module: "globals",
    class: "Set",
    contract: "src/modules/primitives/set/set.vn",
    impl Set {
        fn constructor(ctx: &mut dyn NativeCtx, _this: VmValue) -> VmValue {
            ctx.intern(Value::Set(SetRef::new(HashSet::new())))
        }

        fn add(ctx: &mut dyn NativeCtx, this: VmValue, value: VmValue) {
            if let Some(s) = get_set(ctx, this) {
                let val = ctx.extract(value);
                s.borrow_mut().insert(val);
            }
        }
        fn has(ctx: &mut dyn NativeCtx, this: VmValue, value: VmValue) -> bool {
            match get_set(ctx, this) {
                Some(s) => {
                    let val = ctx.extract(value);
                    s.borrow().contains(&val)
                }
                None => false,
            }
        }
        fn delete(ctx: &mut dyn NativeCtx, this: VmValue, value: VmValue) -> bool {
            match get_set(ctx, this) {
                Some(s) => {
                    let val = ctx.extract(value);
                    s.borrow_mut().remove(&val)
                }
                None => false,
            }
        }
        fn clear(ctx: &mut dyn NativeCtx, this: VmValue) {
            if let Some(s) = get_set(ctx, this) {
                s.borrow_mut().clear();
            }
        }
        fn values(ctx: &mut dyn NativeCtx, this: VmValue) -> Vec<VmValue> {
            match get_set(ctx, this) {
                Some(s) => {
                    let items: Vec<Value> = s.borrow().iter().cloned().collect();
                    items.into_iter().map(|v| ctx.intern(v)).collect()
                }
                None => Vec::new(),
            }
        }
        fn forEach(ctx: &mut dyn NativeCtx, this: VmValue, callback: VmValue) {
            if let Some(s) = get_set(ctx, this) {
                let items: Vec<Value> = s.borrow().iter().cloned().collect();
                for v in items {
                    let v_nv = ctx.intern(v);
                    let _ = ctx.call_vm(callback, &[v_nv, v_nv, this]);
                }
            }
        }
        fn size(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            get_set(ctx, this).map(|s| s.borrow().len() as i64).unwrap_or(0)
        }
    }
}
