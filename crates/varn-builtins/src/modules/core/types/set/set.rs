use varn_op_macros::varn_contract;
use varn_types::value::SetRef;
use varn_types::{NativeCtx, NativeError, Value, VmValue};

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
    contract: "src/modules/core/types/set/set.vn",
    impl Set {
        fn constructor(ctx: &mut dyn NativeCtx, _this: VmValue) -> VmValue {
            ctx.intern(Value::Set(SetRef::new(varn_types::value::ValueSet::default())))
        }

        fn add(ctx: &mut dyn NativeCtx, this: VmValue, value: VmValue) -> Result<(), NativeError> {
            if let Some(s) = get_set(ctx, this) {
                let k = ctx.map_key(value)?;
                s.borrow_mut().insert(k);
                // Identity keys can hold nursery indices; interior-mutability
                // store, so no opcode barrier sees it.
                ctx.collection_write_barrier(this, k.0);
            }
            Ok(())
        }
        fn has(ctx: &mut dyn NativeCtx, this: VmValue, value: VmValue) -> Result<bool, NativeError> {
            match get_set(ctx, this) {
                Some(s) => {
                    let k = ctx.map_key(value)?;
                    Ok(s.borrow().contains(&k))
                }
                None => Ok(false),
            }
        }
        fn delete(ctx: &mut dyn NativeCtx, this: VmValue, value: VmValue) -> Result<bool, NativeError> {
            match get_set(ctx, this) {
                Some(s) => {
                    let k = ctx.map_key(value)?;
                    Ok(s.borrow_mut().remove(&k))
                }
                None => Ok(false),
            }
        }
        fn clear(ctx: &mut dyn NativeCtx, this: VmValue) {
            if let Some(s) = get_set(ctx, this) {
                s.borrow_mut().clear();
            }
        }
        fn values(ctx: &mut dyn NativeCtx, this: VmValue) -> Vec<VmValue> {
            match get_set(ctx, this) {
                Some(s) => s.borrow().iter().map(|k| k.0).collect(),
                None => Vec::new(),
            }
        }
        fn forEach(ctx: &mut dyn NativeCtx, this: VmValue, callback: VmValue) -> Result<(), NativeError> {
            if let Some(s) = get_set(ctx, this) {
                let items: Vec<VmValue> = s.borrow().iter().map(|k| k.0).collect();
                for v in items {
                    ctx.call_vm(callback, &[v, v, this])?;
                }
            }
            Ok(())
        }
        fn size(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            get_set(ctx, this).map(|s| s.borrow().len() as i64).unwrap_or(0)
        }
    }
}
