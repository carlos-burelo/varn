use varn_op_macros::varn_contract;
use varn_types::value::{MapRef, Value};
use varn_types::{NativeCtx, VmValue, VnArray};

pub struct List;
pub struct Stack;
pub struct Queue;
pub struct Record;

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

fn items_to_vec(ctx: &dyn NativeCtx, items: VmValue) -> Vec<VmValue> {
    let len = ctx.array_len(items);
    (0..len)
        .map(|i| ctx.array_get(items, i).unwrap_or_else(VmValue::null))
        .collect()
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
        let map = MapRef::new(varn_types::value::ValueMap::default());
        let entries = ctx.intern(Value::Map(map.clone()));
        ctx.set_field(this, "entries", entries);
        map
    }
}

varn_contract! {
    module: "std:collections",
    class: "List",
    contract: "src/modules/std/collections/collections.vn",
    impl List {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, initial: Option<VnArray>) -> VmValue {
            let items = match initial {
                Some(a) => a.raw(),
                None => ctx.alloc_array(vec![]),
            };
            ctx.set_field(this, "items", items);
            this
        }
        fn length(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            ctx.array_len(get_items(ctx, this)) as i64
        }
        fn add(ctx: &mut dyn NativeCtx, this: VmValue, item: VmValue) {
            let items = ensure_items(ctx, this);
            ctx.array_push(items, item);
        }
        fn push(ctx: &mut dyn NativeCtx, this: VmValue, item: VmValue) {
            let items = ensure_items(ctx, this);
            ctx.array_push(items, item);
        }
        fn pop(ctx: &mut dyn NativeCtx, this: VmValue) -> Option<VmValue> {
            let items = ensure_items(ctx, this);
            ctx.array_pop(items)
        }
        fn get(ctx: &mut dyn NativeCtx, this: VmValue, index: i64) -> VmValue {
            ctx.array_get(get_items(ctx, this), index.max(0) as usize).unwrap_or_else(VmValue::null)
        }
        fn set(ctx: &mut dyn NativeCtx, this: VmValue, index: i64, val: VmValue) {
            let items = ensure_items(ctx, this);
            ctx.array_set(items, index.max(0) as usize, val);
        }
        fn clear(ctx: &mut dyn NativeCtx, this: VmValue) {
            let items = ctx.alloc_array(vec![]);
            ctx.set_field(this, "items", items);
        }
        fn contains(ctx: &mut dyn NativeCtx, this: VmValue, item: VmValue) -> bool {
            let mut found = false;
            ctx.array_for_each(get_items(ctx, this), &mut |v, _| {
                if v == item {
                    found = true;
                }
            });
            found
        }
        fn toArray(ctx: &mut dyn NativeCtx, this: VmValue) -> Vec<VmValue> {
            items_to_vec(ctx, get_items(ctx, this))
        }
    }
}

varn_contract! {
    module: "std:collections",
    class: "Stack",
    contract: "src/modules/std/collections/collections.vn",
    impl Stack {
        fn push(ctx: &mut dyn NativeCtx, this: VmValue, item: VmValue) {
            let items = ensure_items(ctx, this);
            ctx.array_push(items, item);
        }
        fn pop(ctx: &mut dyn NativeCtx, this: VmValue) -> Option<VmValue> {
            let items = ensure_items(ctx, this);
            ctx.array_pop(items)
        }
        fn peek(ctx: &mut dyn NativeCtx, this: VmValue) -> Option<VmValue> {
            let items = get_items(ctx, this);
            let len = ctx.array_len(items);
            if len == 0 {
                None
            } else {
                ctx.array_get(items, len - 1)
            }
        }
        fn size(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            ctx.array_len(get_items(ctx, this)) as i64
        }
        fn isEmpty(ctx: &mut dyn NativeCtx, this: VmValue) -> bool {
            ctx.array_len(get_items(ctx, this)) == 0
        }
    }
}

varn_contract! {
    module: "std:collections",
    class: "Queue",
    contract: "src/modules/std/collections/collections.vn",
    impl Queue {
        fn enqueue(ctx: &mut dyn NativeCtx, this: VmValue, item: VmValue) {
            let items = ensure_items(ctx, this);
            ctx.array_push(items, item);
        }
        fn dequeue(ctx: &mut dyn NativeCtx, this: VmValue) -> Option<VmValue> {
            let items = get_items(ctx, this);
            let len = ctx.array_len(items);
            if len == 0 {
                return None;
            }
            let first = ctx.array_get(items, 0);
            let mut tail = Vec::with_capacity(len.saturating_sub(1));
            for i in 1..len {
                if let Some(v) = ctx.array_get(items, i) {
                    tail.push(v);
                }
            }
            let next = ctx.alloc_array(tail);
            ctx.set_field(this, "items", next);
            first
        }
        fn peek(ctx: &mut dyn NativeCtx, this: VmValue) -> Option<VmValue> {
            ctx.array_get(get_items(ctx, this), 0)
        }
        fn size(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            ctx.array_len(get_items(ctx, this)) as i64
        }
        fn isEmpty(ctx: &mut dyn NativeCtx, this: VmValue) -> bool {
            ctx.array_len(get_items(ctx, this)) == 0
        }
    }
}

varn_contract! {
    module: "std:collections",
    class: "Record",
    contract: "src/modules/std/collections/collections.vn",
    impl Record {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            let map = MapRef::new(varn_types::value::ValueMap::default());
            let entries = ctx.intern(Value::Map(map));
            ctx.set_field(this, "entries", entries);
            this
        }
        fn get(ctx: &mut dyn NativeCtx, this: VmValue, key: VmValue) -> Option<VmValue> {
            let k = ctx.extract(key);
            let found = get_record_entries(ctx, this).and_then(|m| m.borrow().get(&k).cloned());
            found.map(|v| ctx.intern(v))
        }
        fn set(ctx: &mut dyn NativeCtx, this: VmValue, key: VmValue, value: VmValue) {
            let k = ctx.extract(key);
            let v = ctx.extract(value);
            ensure_record_entries(ctx, this).borrow_mut().insert(k, v);
        }
        fn has(ctx: &mut dyn NativeCtx, this: VmValue, key: VmValue) -> bool {
            let k = ctx.extract(key);
            get_record_entries(ctx, this).map(|m| m.borrow().contains_key(&k)).unwrap_or(false)
        }
        fn delete(ctx: &mut dyn NativeCtx, this: VmValue, key: VmValue) -> bool {
            let k = ctx.extract(key);
            get_record_entries(ctx, this).map(|m| m.borrow_mut().remove(&k).is_some()).unwrap_or(false)
        }
        fn keys(ctx: &mut dyn NativeCtx, this: VmValue) -> Vec<VmValue> {
            match get_record_entries(ctx, this) {
                Some(m) => {
                    let ks: Vec<Value> = m.borrow().keys().cloned().collect();
                    ks.into_iter().map(|k| ctx.intern(k)).collect()
                }
                None => Vec::new(),
            }
        }
        fn values(ctx: &mut dyn NativeCtx, this: VmValue) -> Vec<VmValue> {
            match get_record_entries(ctx, this) {
                Some(m) => {
                    let vs: Vec<Value> = m.borrow().values().cloned().collect();
                    vs.into_iter().map(|v| ctx.intern(v)).collect()
                }
                None => Vec::new(),
            }
        }
        fn size(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            get_record_entries(ctx, this).map(|m| m.borrow().len() as i64).unwrap_or(0)
        }
    }
}
