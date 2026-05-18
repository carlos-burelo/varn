use std::collections::HashSet;
#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_getter, varn_method, varn_module};
use varn_types::value::SetRef;
use varn_types::{NativeCtx, Value, VmValue};

fn get_set(ctx: &dyn NativeCtx, this: VmValue) -> Option<SetRef> {
    if let Value::Set(s) = ctx.extract(this) {
        Some(s)
    } else {
        None
    }
}

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_class("Set")]
    pub mod set_class {
        use super::*;

        #[varn_method("constructor")]
        pub fn set_new(
            ctx: &mut dyn NativeCtx,
            _this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let set = SetRef::new(HashSet::new());
            Ok(ctx.intern(Value::Set(set)))
        }

        #[varn_method("add")]
        pub fn set_add(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&val_nv) = args.first() {
                if let Some(s) = get_set(ctx, this) {
                    let val = ctx.extract(val_nv);
                    s.borrow_mut().insert(val);
                    return Ok(this);
                }
            }
            Ok(VmValue::null())
        }

        #[varn_method("has")]
        pub fn set_has(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&val_nv) = args.first() {
                if let Some(s) = get_set(ctx, this) {
                    let val = ctx.extract(val_nv);
                    return Ok(VmValue::from_bool(s.borrow().contains(&val)));
                }
            }
            Ok(VmValue::from_bool(false))
        }

        #[varn_method("delete")]
        pub fn set_delete(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&val_nv) = args.first() {
                if let Some(s) = get_set(ctx, this) {
                    let val = ctx.extract(val_nv);
                    let removed = s.borrow_mut().remove(&val);
                    return Ok(VmValue::from_bool(removed));
                }
            }
            Ok(VmValue::from_bool(false))
        }

        #[varn_method("clear")]
        pub fn set_clear(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(s) = get_set(ctx, this) {
                s.borrow_mut().clear();
            }
            Ok(VmValue::null())
        }

        #[varn_method("values")]
        pub fn set_values(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(s) = get_set(ctx, this) {
                let vals: Vec<VmValue> = s.borrow().iter().map(|v| ctx.intern(v.clone())).collect();
                return Ok(ctx.alloc_array(vals));
            }
            Ok(ctx.alloc_array(vec![]))
        }

        #[varn_method("forEach")]
        pub fn set_for_each(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&cb) = args.first() {
                if let Some(s) = get_set(ctx, this) {
                    let vals: Vec<Value> = s.borrow().iter().cloned().collect();
                    for v in vals {
                        let v_nv = ctx.intern(v);
                        ctx.call_vm(cb, &[v_nv, v_nv, this])?;
                    }
                }
            }
            Ok(VmValue::null())
        }

        #[varn_getter("size")]
        pub fn set_size(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<VmValue, String> {
            if let Some(s) = get_set(ctx, this) {
                return Ok(VmValue::from_int(s.borrow().len() as i64));
            }
            Ok(VmValue::from_int(0))
        }
    }
}
