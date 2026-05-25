#![allow(non_upper_case_globals)]
use std::collections::HashMap;
#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_constructor, varn_getter, varn_method, varn_module};
use varn_types::value::{MapRef, Value};
use varn_types::{NativeCtx, VmValue};

#[varn_module("std:collections")]
pub(crate) mod dispatch {
    use super::*;

    fn get_items(ctx: &dyn NativeCtx, this: VmValue) -> VmValue {
        ctx.get_field(this, "items").unwrap_or(VmValue::null())
    }

    fn ensure_items(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
        if let Some(items) = ctx.get_field(this, "items") {
            items
        } else {
            let items = ctx.alloc_array(vec![]);
            ctx.set_field(this, "items", items);
            items
        }
    }

    fn get_record_entries(ctx: &dyn NativeCtx, this: VmValue) -> Option<MapRef> {
        ctx.get_field(this, "entries")
            .and_then(|v| match ctx.extract(v) {
                Value::Map(map) => Some(map),
                _ => None,
            })
    }

    fn ensure_record_entries(ctx: &mut dyn NativeCtx, this: VmValue) -> MapRef {
        if let Some(map) = get_record_entries(ctx, this) {
            map
        } else {
            let map = MapRef::new(HashMap::new());
            let entries = ctx.intern(Value::Map(map.clone()));
            ctx.set_field(this, "entries", entries);
            map
        }
    }

    #[varn_class("List")]
    #[allow(non_upper_case_globals)]
    pub mod list_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<(), String> {
            let items = args
                .first()
                .copied()
                .filter(|v| ctx.is_array(*v))
                .unwrap_or_else(|| ctx.alloc_array(vec![]));
            ctx.set_field(this, "items", items);
            Ok(())
        }

        #[varn_getter("length")]
        pub fn length(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<VmValue, String> {
            Ok(VmValue::from_int(ctx.array_len(get_items(ctx, this)) as i64))
        }

        #[varn_method("add")]
        pub fn add(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&item) = args.first() {
                let items = ensure_items(ctx, this);
                ctx.array_push(items, item);
            }
            Ok(VmValue::null())
        }

        #[varn_method("push")]
        pub fn push(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&item) = args.first() {
                let items = ensure_items(ctx, this);
                ctx.array_push(items, item);
            }
            Ok(VmValue::null())
        }

        #[varn_method("pop")]
        pub fn pop(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let items = ensure_items(ctx, this);
            Ok(ctx.array_pop(items).unwrap_or(VmValue::null()))
        }

        #[varn_method("get")]
        pub fn get(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            let idx = args.first().copied().unwrap_or(VmValue::null());
            let idx = if let varn_types::Value::Int(n) = ctx.extract(idx) {
                n
            } else {
                0
            };
            Ok(ctx
                .array_get(get_items(ctx, this), idx.max(0) as usize)
                .unwrap_or(VmValue::null()))
        }

        #[varn_method("set")]
        pub fn set(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            let idx = args.first().copied().unwrap_or(VmValue::null());
            let val = args.get(1).copied().unwrap_or(VmValue::null());
            let idx = if let varn_types::Value::Int(n) = ctx.extract(idx) {
                n
            } else {
                0
            };
            let items = ensure_items(ctx, this);
            ctx.array_set(items, idx.max(0) as usize, val);
            Ok(VmValue::null())
        }

        #[varn_method("clear")]
        pub fn clear(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let items = ctx.alloc_array(vec![]);
            ctx.set_field(this, "items", items);
            Ok(VmValue::null())
        }

        #[varn_method("contains")]
        pub fn contains(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            let target = args.first().copied().unwrap_or(VmValue::null());
            let mut found = false;
            ctx.array_for_each(get_items(ctx, this), &mut |item, _| {
                if item == target {
                    found = true;
                }
            });
            Ok(VmValue::from_bool(found))
        }

        #[varn_method("toArray")]
        pub fn to_array(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            Ok(get_items(ctx, this))
        }
    }

    #[varn_class("Stack")]
    #[allow(non_upper_case_globals)]
    pub mod stack_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<(), String> {
            let items = ctx.alloc_array(vec![]);
            ctx.set_field(this, "items", items);
            Ok(())
        }

        #[varn_method("push")]
        pub fn push(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&item) = args.first() {
                let items = ensure_items(ctx, this);
                ctx.array_push(items, item);
            }
            Ok(VmValue::null())
        }

        #[varn_method("pop")]
        pub fn pop(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let items = ensure_items(ctx, this);
            Ok(ctx.array_pop(items).unwrap_or(VmValue::null()))
        }

        #[varn_method("peek")]
        pub fn peek(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let items = get_items(ctx, this);
            let len = ctx.array_len(items);
            Ok(if len == 0 {
                VmValue::null()
            } else {
                ctx.array_get(items, len - 1).unwrap_or(VmValue::null())
            })
        }

        #[varn_getter("size")]
        pub fn size(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<VmValue, String> {
            Ok(VmValue::from_int(ctx.array_len(get_items(ctx, this)) as i64))
        }

        #[varn_getter("isEmpty")]
        pub fn is_empty(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<VmValue, String> {
            Ok(VmValue::from_bool(ctx.array_len(get_items(ctx, this)) == 0))
        }
    }

    #[varn_class("Queue")]
    #[allow(non_upper_case_globals)]
    pub mod queue_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<(), String> {
            let items = ctx.alloc_array(vec![]);
            ctx.set_field(this, "items", items);
            Ok(())
        }

        #[varn_method("enqueue")]
        pub fn enqueue(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&item) = args.first() {
                let items = ensure_items(ctx, this);
                ctx.array_push(items, item);
            }
            Ok(VmValue::null())
        }

        #[varn_method("dequeue")]
        pub fn dequeue(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let items = get_items(ctx, this);
            let len = ctx.array_len(items);
            if len == 0 {
                return Ok(VmValue::null());
            }
            let first = ctx.array_get(items, 0).unwrap_or(VmValue::null());
            let mut tail = Vec::with_capacity(len.saturating_sub(1));
            for i in 1..len {
                if let Some(v) = ctx.array_get(items, i) {
                    tail.push(v);
                }
            }
            let next = ctx.alloc_array(tail);
            ctx.set_field(this, "items", next);
            Ok(first)
        }

        #[varn_method("peek")]
        pub fn peek(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            Ok(ctx
                .array_get(get_items(ctx, this), 0)
                .unwrap_or(VmValue::null()))
        }

        #[varn_getter("size")]
        pub fn size(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<VmValue, String> {
            Ok(VmValue::from_int(ctx.array_len(get_items(ctx, this)) as i64))
        }

        #[varn_getter("isEmpty")]
        pub fn is_empty(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<VmValue, String> {
            Ok(VmValue::from_bool(ctx.array_len(get_items(ctx, this)) == 0))
        }
    }

    #[varn_class("Record")]
    #[allow(non_upper_case_globals)]
    pub mod record_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<(), String> {
            let map = MapRef::new(HashMap::new());
            let entries = ctx.intern(Value::Map(map));
            ctx.set_field(this, "entries", entries);
            Ok(())
        }

        #[varn_method("get")]
        pub fn get(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            let key = args.first().copied().unwrap_or(VmValue::null());
            let key = ctx.extract(key);
            Ok(get_record_entries(ctx, this)
                .and_then(|m| m.borrow().get(&key).cloned())
                .map(|v| ctx.intern(v))
                .unwrap_or(VmValue::null()))
        }

        #[varn_method("set")]
        pub fn set(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let (Some(&key_nv), Some(&value_nv)) = (args.first(), args.get(1)) {
                let key = ctx.extract(key_nv);
                let value = ctx.extract(value_nv);
                ensure_record_entries(ctx, this)
                    .borrow_mut()
                    .insert(key, value);
            }
            Ok(VmValue::null())
        }

        #[varn_method("has")]
        pub fn has(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            let key = args.first().copied().unwrap_or(VmValue::null());
            let key = ctx.extract(key);
            Ok(VmValue::from_bool(
                get_record_entries(ctx, this)
                    .map(|m| m.borrow().contains_key(&key))
                    .unwrap_or(false),
            ))
        }

        #[varn_method("delete")]
        pub fn delete(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            let key = args.first().copied().unwrap_or(VmValue::null());
            let key = ctx.extract(key);
            Ok(VmValue::from_bool(
                get_record_entries(ctx, this)
                    .map(|m| m.borrow_mut().remove(&key).is_some())
                    .unwrap_or(false),
            ))
        }

        #[varn_method("keys")]
        pub fn keys(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(m) = get_record_entries(ctx, this) {
                let keys: Vec<_> = m.borrow().keys().cloned().collect();
                let items = keys.into_iter().map(|k| ctx.intern(k)).collect();
                return Ok(ctx.alloc_array(items));
            }
            Ok(ctx.alloc_array(vec![]))
        }

        #[varn_method("values")]
        pub fn values(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(m) = get_record_entries(ctx, this) {
                let values: Vec<_> = m.borrow().values().cloned().collect();
                let items = values.into_iter().map(|v| ctx.intern(v)).collect();
                return Ok(ctx.alloc_array(items));
            }
            Ok(ctx.alloc_array(vec![]))
        }

        #[varn_getter("size")]
        pub fn size(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<VmValue, String> {
            Ok(VmValue::from_int(
                get_record_entries(ctx, this)
                    .map(|m| m.borrow().len() as i64)
                    .unwrap_or(0),
            ))
        }
    }
}
