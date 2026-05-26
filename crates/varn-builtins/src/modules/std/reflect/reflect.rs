use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_method, varn_module, varn_static};
use varn_types::value::{ClassObj, ObjData, ObjRef};
use varn_types::{NativeCtx, Value, VmValue};

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

fn meta_key_id(ctx: &dyn NativeCtx, this: VmValue) -> Option<String> {
    if let Value::Object(o) = ctx.extract(this) {
        let guard = o.borrow();
        if let Some(nv) = guard.inner.get("metaId") {
            return ctx.str_owned(nv);
        }
    }
    None
}

static META_KEY_COUNTER: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static META_KEY_CLASS_CACHE: RefCell<Option<Rc<ClassObj>>> = RefCell::new(None);
}

fn get_meta_key_class() -> Rc<ClassObj> {
    META_KEY_CLASS_CACHE.with(|c| {
        let mut guard = c.borrow_mut();
        if let Some(ref cls) = *guard {
            return cls.clone();
        }
        let cls = build_meta_key_class_obj();
        *guard = Some(cls.clone());
        cls
    })
}

fn build_meta_key_class_obj() -> Rc<ClassObj> {
    let cls = ClassObj::new_native_rc("MetaKey");
    cls.add_static("create", Value::native(meta_key_create_raw, "create"));
    cls.add_method("set", Value::native(meta_key_set_raw, "set"));
    cls.add_method("get", Value::native(meta_key_get_raw, "get"));
    cls.add_method("has", Value::native(meta_key_has_raw, "has"));
    cls
}

fn meta_key_create_raw(ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
    let id = META_KEY_COUNTER.fetch_add(1, Ordering::Relaxed);
    let key_name = format!("meta_{}", id);
    let cls = get_meta_key_class();
    let mut obj = ObjData::new_instance(cls);
    let key_nv = ctx.alloc_str_owned(key_name);
    obj.inner.insert(Rc::from("metaId"), key_nv);
    let nv = ctx.intern(Value::Object(ObjRef(Rc::new(RefCell::new(obj)))));
    Ok(nv)
}

fn meta_key_set_raw(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    if let (Some(&this), Some(&target), Some(&val)) = (args.first(), args.get(1), args.get(2)) {
        if let Some(meta_id) = meta_key_id(ctx, this) {
            let target_k = target_key(ctx, target);
            with_metadata(|m| {
                m.entry(target_k).or_default().insert(meta_id, val);
            });
        }
    }
    Ok(VmValue::null())
}

fn meta_key_get_raw(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    if let (Some(&this), Some(&target)) = (args.first(), args.get(1)) {
        if let Some(meta_id) = meta_key_id(ctx, this) {
            let target_k = target_key(ctx, target);
            let res = with_metadata(|m| {
                m.get(&target_k)
                    .and_then(|m| m.get(&meta_id))
                    .copied()
                    .unwrap_or(VmValue::null())
            });
            return Ok(res);
        }
    }
    Ok(VmValue::null())
}

fn meta_key_has_raw(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    if let (Some(&this), Some(&target)) = (args.first(), args.get(1)) {
        if let Some(meta_id) = meta_key_id(ctx, this) {
            let target_k = target_key(ctx, target);
            let found = with_metadata(|m| {
                m.get(&target_k)
                    .map(|m| m.contains_key(&meta_id))
                    .unwrap_or(false)
            });
            return Ok(VmValue::from_bool(found));
        }
    }
    Ok(VmValue::from_bool(false))
}

#[varn_module("std:reflect")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn("defineMetadata")]
    pub fn define_metadata(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let key = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let val = args.get(1).cloned().unwrap_or(VmValue::null());
        let target = args.get(2).map(|&v| target_key(ctx, v)).unwrap_or_default();
        with_metadata(|m| {
            m.entry(target).or_default().insert(key, val);
        });
        Ok(VmValue::null())
    }

    #[varn_fn("getMetadata")]
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

    #[varn_fn("hasMetadata")]
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

    #[varn_class("MetaKey")]
    pub mod meta_key_class {
        use super::*;

        #[varn_static("create")]
        pub fn create(ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
            let id = META_KEY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let key_name = format!("meta_{}", id);

            let cls = get_meta_key_class();
            let mut obj = ObjData::new_instance(cls);
            let key_nv = ctx.alloc_str_owned(key_name);
            obj.inner.insert(Rc::from("metaId"), key_nv);
            let nv = ctx.intern(Value::Object(ObjRef(Rc::new(RefCell::new(obj)))));
            Ok(nv)
        }

        #[varn_method("set")]
        pub fn set_meta(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            let target = args.first().copied().unwrap_or(VmValue::null());
            let val = args.get(1).copied().unwrap_or(VmValue::null());
            if let Some(meta_id) = ctx.str_owned(
                ctx.get_field(this, "metaId")
                    .unwrap_or(VmValue::null()),
            ) {
                let target_k = if target.is_heap() {
                    format!("heap:{:x}", target.as_heap_idx())
                } else if target.is_int() {
                    format!("int:{}", target.as_int())
                } else {
                    ctx.str_repr(target)
                };
                with_metadata(|m| {
                    m.entry(target_k).or_default().insert(meta_id, val);
                });
            }
            Ok(VmValue::null())
        }

        #[varn_method("get")]
        pub fn get_meta(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            let target = args.first().copied().unwrap_or(VmValue::null());
            if let Some(meta_id) = ctx.str_owned(
                ctx.get_field(this, "metaId")
                    .unwrap_or(VmValue::null()),
            ) {
                let target_k = if target.is_heap() {
                    format!("heap:{:x}", target.as_heap_idx())
                } else if target.is_int() {
                    format!("int:{}", target.as_int())
                } else {
                    ctx.str_repr(target)
                };
                let res = with_metadata(|m| {
                    m.get(&target_k)
                        .and_then(|m| m.get(&meta_id))
                        .copied()
                        .unwrap_or(VmValue::null())
                });
                return Ok(res);
            }
            Ok(VmValue::null())
        }

        #[varn_method("has")]
        pub fn has_meta(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            let target = args.first().copied().unwrap_or(VmValue::null());
            if let Some(meta_id) = ctx.str_owned(
                ctx.get_field(this, "metaId")
                    .unwrap_or(VmValue::null()),
            ) {
                let target_k = if target.is_heap() {
                    format!("heap:{:x}", target.as_heap_idx())
                } else if target.is_int() {
                    format!("int:{}", target.as_int())
                } else {
                    ctx.str_repr(target)
                };
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
}
