use std::collections::hash_map::Entry;
use std::rc::Rc;
use std::sync::Arc;
use crate::error::{RuntimeError, VmResult};
use crate::closure::{VmClosure, VmClosurePayload, VmValueRef};
use crate::nursery::{is_nursery_idx, old_idx_raw, pack_old_idx};
use crate::value::VmValue;
use varn_types::{
    value::{
        FrozenModuleObj, MapKey, ModuleObj, ObjRef, RangeData, RuntimeSymbol,
    },
    NativeFn, RuntimeString, Value, VmArray,
};
use super::obj::HeapObj;
use super::str::{ascii_flag, HeapStr, INLINE_STR_CAP};
use super::structs::HeapInner;

const SLICE_LEN_MASK: u32 = 0x3FFF_FFFF;

#[inline]
pub(super) fn alloc_into(
    objects: &mut Vec<Option<HeapObj>>,
    free: &mut Vec<u32>,
    alloc_count: &mut u64,
    gc_alloc_since_collect: &mut u64,
    obj: HeapObj,
) -> u32 {
    *alloc_count += 1;
    *gc_alloc_since_collect += 1;
    if let Some(idx) = free.pop() {
        objects[idx as usize] = Some(obj);
        idx
    } else {
        let idx = objects.len() as u32;
        objects.push(Some(obj));
        idx
    }
}

impl HeapInner {
    #[inline]
    pub(crate) fn alloc_native_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::NativeFn(name, f)))
    }

    pub(crate) fn alloc(&mut self, obj: HeapObj) -> u32 {
        if let Some(h) = &self.hotspot {
            h.borrow_mut().record_alloc(obj.tag().name());
        }
        let mut obj = obj;
        match obj {
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Object(_)
            | HeapObj::VmClosure(_)
            | HeapObj::BoundMethod(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::Range(_)
            | HeapObj::Char(_)
            | HeapObj::Symbol(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_) => match self.nursery.try_alloc(obj) {
                Ok(ni) => return ni,
                Err(returned_obj) => {
                    obj = returned_obj;
                }
            },
            _ => {}
        }
        let track = Self::needs_minor_scan(&obj);
        let identity = Self::identity_key(&obj);
        let raw = alloc_into(
            &mut self.objects,
            &mut self.free,
            &mut self.alloc_count,
            &mut self.gc_alloc_since_collect,
            obj,
        );
        if track {
            self.scan_roots.push(raw);
        }
        if let Some(key) = identity {
            self.identity_index.insert(key, pack_old_idx(raw));
        }
        pack_old_idx(raw)
    }

    pub(crate) fn alloc_raw(&mut self, obj: HeapObj) -> u32 {
        self.alloc_count += 1;
        let track = Self::needs_minor_scan(&obj);
        let identity = Self::identity_key(&obj);
        let idx = if let Some(idx) = self.free.pop() {
            self.objects[idx as usize] = Some(obj);
            idx
        } else {
            let idx = self.objects.len() as u32;
            self.objects.push(Some(obj));
            idx
        };
        if track {
            self.scan_roots.push(idx);
        }
        if let Some(key) = identity {
            self.identity_index.insert(key, pack_old_idx(idx));
        }
        idx
    }

    #[inline(always)]
    pub(crate) fn get_by_idx(&self, idx: u32) -> Option<&HeapObj> {
        if is_nursery_idx(idx) {
            self.nursery.get(idx)
        } else {
            self.objects.get(old_idx_raw(idx) as usize)?.as_ref()
        }
    }

    #[inline(always)]
    pub(crate) fn get_by_idx_mut(&mut self, idx: u32) -> Option<&mut HeapObj> {
        if is_nursery_idx(idx) {
            self.nursery.get_mut(idx)
        } else {
            self.objects.get_mut(old_idx_raw(idx) as usize)?.as_mut()
        }
    }

    #[inline(always)]
    pub(crate) fn get(&self, idx: u32) -> Option<&HeapObj> {
        self.get_by_idx(idx)
    }

    #[inline(always)]
    pub(crate) fn get_closure(&self, idx: u32) -> Option<&VmClosure> {
        let obj = if is_nursery_idx(idx) {
            self.nursery.get(idx)?
        } else {
            self.objects.get(old_idx_raw(idx) as usize)?.as_ref()?
        };
        match obj {
            HeapObj::VmClosure(c) => Some(&**c),
            _ => None,
        }
    }

    #[inline(always)]
    pub(crate) fn get_mut(&mut self, idx: u32) -> Option<&mut HeapObj> {
        self.get_by_idx_mut(idx)
    }

    #[inline(always)]
    pub(crate) fn get_raw(&self, raw_old_idx: u32) -> Option<&HeapObj> {
        self.objects.get(raw_old_idx as usize)?.as_ref()
    }

    #[inline(always)]
    pub(crate) fn get_raw_mut(&mut self, raw_old_idx: u32) -> Option<&mut HeapObj> {
        self.objects.get_mut(raw_old_idx as usize)?.as_mut()
    }

    pub fn intern(&mut self, val: Value) -> VmValue {
        match val {
            Value::Null => VmValue::null(),
            Value::Bool(b) => VmValue::from_bool(b),
            Value::Int(n) => VmValue::from_int(n),
            Value::Float(f) => VmValue::from_f64(f),
            Value::Char(c) => {
                let packed = match self.char_interner.entry(c) {
                    Entry::Occupied(e) => *e.get(),
                    Entry::Vacant(e) => *e.insert(pack_old_idx(alloc_into(
                        &mut self.objects,
                        &mut self.free,
                        &mut self.alloc_count,
                        &mut self.gc_alloc_since_collect,
                        HeapObj::Char(c),
                    ))),
                };
                VmValue::from_heap_idx(packed)
            }
            Value::Str(s) => self.alloc_str(s),
            Value::BigInt(n) => {
                let n_val = *n;
                let packed = match self.bigint_interner.entry(n_val) {
                    Entry::Occupied(e) => *e.get(),
                    Entry::Vacant(e) => *e.insert(pack_old_idx(alloc_into(
                        &mut self.objects,
                        &mut self.free,
                        &mut self.alloc_count,
                        &mut self.gc_alloc_since_collect,
                        HeapObj::BigInt(n_val),
                    ))),
                };
                VmValue::from_heap_idx(packed)
            }
            Value::Decimal(d) => {
                let d_val = *d;
                let packed = match self.decimal_interner.entry(d_val) {
                    Entry::Occupied(e) => *e.get(),
                    Entry::Vacant(e) => *e.insert(pack_old_idx(alloc_into(
                        &mut self.objects,
                        &mut self.free,
                        &mut self.alloc_count,
                        &mut self.gc_alloc_since_collect,
                        HeapObj::Decimal(Box::new(d_val)),
                    ))),
                };
                VmValue::from_heap_idx(packed)
            }
            Value::Array(a) => {
                let guard = a.borrow();
                let vm_items: Vec<VmValue> = guard.iter().map(|v| self.intern(v.clone())).collect();
                let va = VmArray::from_items(vm_items);
                let idx = self.alloc(HeapObj::Array(va));
                VmValue::from_heap_idx(idx)
            }
            Value::Object(o) => {
                if let Some(&idx) = self.object_interner.get(&o) {
                    VmValue::from_heap_idx(idx)
                } else {
                    let idx = self.alloc(HeapObj::Object(o.clone()));
                    self.object_interner.insert(o, idx);
                    VmValue::from_heap_idx(idx)
                }
            }
            Value::Class(c) => VmValue::from_heap_idx(self.alloc(HeapObj::Class(c))),
            Value::NativeFn(b) => VmValue::from_heap_idx(self.alloc(HeapObj::NativeFn(b.1, b.0))),
            Value::BoundMethod(b) => VmValue::from_heap_idx(self.alloc(HeapObj::BoundMethod(b))),
            Value::Map(m) => {
                if let Some(&idx) = self.map_interner.get(&m) {
                    VmValue::from_heap_idx(idx)
                } else {
                    let idx = self.alloc(HeapObj::Map(m.clone()));
                    self.map_interner.insert(m, idx);
                    VmValue::from_heap_idx(idx)
                }
            }
            Value::Set(s) => {
                if let Some(&idx) = self.set_interner.get(&s) {
                    VmValue::from_heap_idx(idx)
                } else {
                    let idx = self.alloc(HeapObj::Set(s.clone()));
                    self.set_interner.insert(s, idx);
                    VmValue::from_heap_idx(idx)
                }
            }
            Value::Task(t) => VmValue::from_heap_idx(self.alloc(HeapObj::Task(t))),
            Value::TaskHandle(t) => VmValue::from_heap_idx(self.alloc(HeapObj::TaskHandle(t))),
            Value::Range(r) => VmValue::from_heap_idx(self.alloc(HeapObj::Range(*r))),
            Value::Symbol(s) => self.alloc_symbol(s),
            Value::EnumVariant(e) => VmValue::from_heap_idx(self.alloc(HeapObj::EnumVariant(e))),
            Value::Spread(v) => {
                let inner = self.intern(*v);
                VmValue::from_heap_idx(self.alloc(HeapObj::Spread(inner)))
            }
            Value::Generator(g) => VmValue::from_heap_idx(self.alloc(HeapObj::Generator(g))),
            Value::AsyncQueue(q) => VmValue::from_heap_idx(self.alloc(HeapObj::AsyncQueue(q))),
            Value::VmValue(payload) => {
                if let Some(vref) = payload.as_any().downcast_ref::<VmValueRef>() {
                    vref.0
                } else if let Some(wrapper) = payload.as_any().downcast_ref::<VmClosurePayload>() {
                    VmValue::from_heap_idx(self.alloc(HeapObj::VmClosure(wrapper.0.clone())))
                } else {
                    VmValue::from_heap_idx(self.alloc(HeapObj::VmValue(payload)))
                }
            }
            Value::Module(m) => VmValue::from_heap_idx(self.alloc(HeapObj::Module(m))),
        }
    }

    #[track_caller]
    pub(crate) fn extract(&self, nv: VmValue) -> Value {
        match self.extract_val(nv) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "[GC DEBUG] heap.extract failed on nv=0x{:016x} (is_heap={}, heap_idx={})",
                    nv.0,
                    nv.is_heap(),
                    if nv.is_heap() {
                        nv.as_heap_idx() as i64
                    } else {
                        -1
                    }
                );
                eprintln!(
                    "[GC DEBUG] Caller location: {}",
                    std::panic::Location::caller()
                );
                panic!("heap.extract: dangling or corrupted heap reference: {e}");
            }
        }
    }

    pub(crate) fn extract_val(&self, nv: VmValue) -> VmResult<Value> {
        if nv.is_null() {
            return Ok(Value::Null);
        }
        if nv.is_bool() {
            return Ok(Value::Bool(nv.as_bool()));
        }
        if nv.is_int() {
            return Ok(Value::Int(nv.as_int()));
        }
        if nv.is_f64() {
            return Ok(Value::Float(nv.as_f64()));
        }
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            let s = nv.sso_as_str(&mut buf);
            return Ok(Value::Str(Rc::from(s)));
        }
        if nv.is_heap() {
            let obj = self
                .get_by_idx(nv.as_heap_idx())
                .ok_or_else(|| RuntimeError::new("invalid heap ref"))?;
            return Ok(match obj {
                HeapObj::Str(s) => Value::Str(s.to_shared()),
                HeapObj::Array(a) | HeapObj::Tuple(a) => {
                    let val_items: Vec<Value> = (0..a.len())
                        .map(|i| self.extract(a.get_vm(i).unwrap()))
                        .collect();
                    Value::Array(varn_types::value::ArrayRef::new(val_items))
                }
                HeapObj::Object(o) | HeapObj::Record(o) => Value::Object(o.clone()),
                HeapObj::VmClosure(c) => Value::VmValue(Box::new(VmClosurePayload(c.clone()))),
                HeapObj::Class(c) => Value::Class(c.clone()),
                HeapObj::NativeFn(name, f) => Value::NativeFn(Box::new((*f, *name))),
                HeapObj::BoundMethod(m) => Value::BoundMethod(m.clone()),
                HeapObj::Map(m) => Value::Map(m.clone()),
                HeapObj::Set(s) => Value::Set(s.clone()),
                HeapObj::Task(t) => Value::Task(t.clone()),
                HeapObj::TaskHandle(t) => Value::TaskHandle(t.clone()),
                HeapObj::Range(r) => Value::Range(Box::new(r.clone())),
                HeapObj::Symbol(s) => Value::Symbol(s.clone()),
                HeapObj::EnumVariant(data) => Value::EnumVariant(data.clone()),
                HeapObj::BigInt(n) => Value::BigInt(Box::new(*n)),
                HeapObj::Decimal(d) => Value::Decimal(d.clone()),
                HeapObj::Char(c) => Value::Char(*c),
                HeapObj::Generator(g) => Value::Generator(g.clone()),
                HeapObj::AsyncQueue(q) => Value::AsyncQueue(q.clone()),
                HeapObj::Spread(inner) => Value::Spread(Box::new(self.extract(*inner))),
                HeapObj::VmValue(payload) => Value::VmValue(payload.clone_payload()),
                HeapObj::Module(m) => Value::Module(m.clone()),
                HeapObj::Buffer(_) => Value::VmValue(Box::new(VmValueRef(nv))),
                HeapObj::FrozenModule(_) => Value::Null,
            });
        }
        Ok(Value::Null)
    }

    pub(crate) fn alloc_vm_buffer(&mut self, buf: varn_types::VmBuffer) -> VmValue {
        let idx = self.alloc(HeapObj::Buffer(buf));
        VmValue::from_heap_idx(idx)
    }

    pub(crate) fn alloc_str(&mut self, s: impl AsRef<str>) -> VmValue {
        let s_ref = s.as_ref();
        if let Some(sso) = VmValue::try_from_sso(s_ref) {
            return sso;
        }

        let rs: RuntimeString = Rc::from(s_ref);
        if let Some(&packed) = self.string_interner.get(&rs) {
            let raw = old_idx_raw(packed);
            if self
                .objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
            {
                return VmValue::from_heap_idx(packed);
            }
            self.string_interner.remove(&rs);
        }

        let idx = match self
            .nursery
            .try_alloc(HeapObj::Str(HeapStr::shared(rs.clone())))
        {
            Ok(ni) => ni,
            Err(obj) => {
                let oi = alloc_into(
                    &mut self.objects,
                    &mut self.free,
                    &mut self.alloc_count,
                    &mut self.gc_alloc_since_collect,
                    obj,
                );
                let packed = pack_old_idx(oi);
                self.string_interner.insert(rs, packed);
                packed
            }
        };
        VmValue::from_heap_idx(idx)
    }

    pub(crate) fn alloc_str_interned(&mut self, s: impl AsRef<str>) -> VmValue {
        let s_ref = s.as_ref();
        if let Some(sso) = VmValue::try_from_sso(s_ref) {
            return sso;
        }
        if let Some(&packed) = self.string_interner.get(s_ref) {
            let raw = old_idx_raw(packed);
            if self
                .objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
            {
                return VmValue::from_heap_idx(packed);
            }
            self.string_interner.remove(s_ref);
        }
        let rs: RuntimeString = Rc::from(s_ref);
        let oi = alloc_into(
            &mut self.objects,
            &mut self.free,
            &mut self.alloc_count,
            &mut self.gc_alloc_since_collect,
            HeapObj::Str(HeapStr::shared(rs.clone())),
        );
        let packed = pack_old_idx(oi);
        self.string_interner.insert(rs, packed);
        VmValue::from_heap_idx(packed)
    }

    pub(crate) fn alloc_str_dynamic(&mut self, s: impl AsRef<str>) -> VmValue {
        let s_ref = s.as_ref();
        if let Some(sso) = VmValue::try_from_sso(s_ref) {
            return sso;
        }
        if s_ref.len() <= INLINE_STR_CAP {
            return self.alloc_str_view(HeapStr::inline(s_ref));
        }

        let rs: RuntimeString = Rc::from(s_ref);
        let idx = match self.nursery.try_alloc(HeapObj::Str(HeapStr::shared(rs))) {
            Ok(ni) => ni,
            Err(obj) => pack_old_idx(alloc_into(
                &mut self.objects,
                &mut self.free,
                &mut self.alloc_count,
                &mut self.gc_alloc_since_collect,
                obj,
            )),
        };
        VmValue::from_heap_idx(idx)
    }

    pub(crate) fn alloc_substring(&mut self, handle: &HeapStr, bs: usize, be: usize) -> VmValue {
        let sub = &handle.as_str()[bs..be];
        if let Some(sso) = VmValue::try_from_sso(sub) {
            return sso;
        }
        let flag = if handle.is_ascii_cached() {
            ascii_flag::YES
        } else {
            ascii_flag::UNKNOWN
        };
        let len = be - bs;
        if len as u64 <= SLICE_LEN_MASK as u64 {
            match handle {
                HeapStr::Shared(rc, _) => {
                    let hs = HeapStr::slice_of(Rc::clone(rc), bs, len, flag);
                    return self.alloc_str_view(hs);
                }
                HeapStr::Slice { src, off, .. } => {
                    let hs = HeapStr::slice_of(Rc::clone(src), *off as usize + bs, len, flag);
                    return self.alloc_str_view(hs);
                }
                HeapStr::Ext { .. } | HeapStr::Inline { .. } => {}
            }
        }
        self.alloc_str_dynamic(sub)
    }

    pub(crate) fn alloc_str_view(&mut self, hs: HeapStr) -> VmValue {
        let idx = match self.nursery.try_alloc(HeapObj::Str(hs)) {
            Ok(ni) => ni,
            Err(obj) => pack_old_idx(alloc_into(
                &mut self.objects,
                &mut self.free,
                &mut self.alloc_count,
                &mut self.gc_alloc_since_collect,
                obj,
            )),
        };
        VmValue::from_heap_idx(idx)
    }

    pub(crate) fn alloc_symbol(&mut self, s: RuntimeSymbol) -> VmValue {
        let packed = match self.symbol_interner.entry(s.clone()) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => {
                let packed = pack_old_idx(alloc_into(
                    &mut self.objects,
                    &mut self.free,
                    &mut self.alloc_count,
                    &mut self.gc_alloc_since_collect,
                    HeapObj::Symbol(s),
                ));
                *e.insert(packed)
            }
        };
        VmValue::from_heap_idx(packed)
    }

    pub(crate) fn alloc_array_vm(&mut self, items: Vec<VmValue>) -> VmValue {
        let va = VmArray::from_items(items);
        VmValue::from_heap_idx(self.alloc(HeapObj::Array(va)))
    }

    pub(crate) fn alloc_tuple_vm(&mut self, items: Vec<VmValue>) -> VmValue {
        let va = VmArray::from_items(items);
        VmValue::from_heap_idx(self.alloc(HeapObj::Tuple(va)))
    }

    pub(crate) fn alloc_array(&mut self, items: Vec<Value>) -> VmValue {
        let vm_items: Vec<VmValue> = items.into_iter().map(|v| self.intern(v)).collect();
        self.alloc_array_vm(vm_items)
    }

    pub(crate) fn alloc_object(&mut self) -> VmValue {
        let oref = ObjRef::empty();
        VmValue::from_heap_idx(self.alloc(HeapObj::Object(oref)))
    }

    pub(crate) fn alloc_object_with_shape(
        &mut self,
        shape: &Rc<varn_types::Shape>,
        values: Vec<VmValue>,
    ) -> VmValue {
        let oref = ObjRef::with_shape(Rc::clone(shape), values);
        VmValue::from_heap_idx(self.alloc(HeapObj::Object(oref)))
    }

    pub(crate) fn alloc_record_with_shape(
        &mut self,
        shape: &Rc<varn_types::Shape>,
        values: Vec<VmValue>,
    ) -> VmValue {
        let oref = ObjRef::with_shape(Rc::clone(shape), values);
        VmValue::from_heap_idx(self.alloc(HeapObj::Record(oref)))
    }
    
    pub(crate) fn make_int(&mut self, n: i64) -> VmValue {
        VmValue::from_int(n)
    }

    pub(crate) fn lookup_str_map_key(&self, s: &str) -> Option<MapKey> {
        if let Some(sso) = VmValue::try_from_sso(s) {
            return Some(MapKey(sso));
        }
        self.string_interner
            .get(s)
            .map(|&packed| MapKey(VmValue::from_heap_idx(packed)))
    }

    pub(crate) fn lookup_map_key(&self, v: VmValue) -> Option<MapKey> {
        if v.is_f64() {
            if v.as_f64() == 0.0 {
                return Some(MapKey(VmValue::from_f64(0.0)));
            }
            return Some(MapKey(v));
        }
        if !v.is_heap() {
            return Some(MapKey(v));
        }
        match self.get_by_idx(v.as_heap_idx()) {
            Some(HeapObj::Str(s)) => self.lookup_str_map_key(s.as_str()),
            Some(HeapObj::Char(c)) => self
                .char_interner
                .get(c)
                .map(|&p| MapKey(VmValue::from_heap_idx(p))),
            Some(HeapObj::BigInt(b)) => self
                .bigint_interner
                .get(b)
                .map(|&p| MapKey(VmValue::from_heap_idx(p))),
            Some(HeapObj::Decimal(d)) => self
                .decimal_interner
                .get(d)
                .map(|&p| MapKey(VmValue::from_heap_idx(p))),
            _ => Some(MapKey(v)),
        }
    }

    pub(crate) fn canonical_map_key(&mut self, v: VmValue) -> MapKey {
        if v.is_f64() {
            if v.as_f64() == 0.0 {
                return MapKey(VmValue::from_f64(0.0));
            }
            return MapKey(v);
        }
        if !v.is_heap() {
            return MapKey(v);
        }
        enum Canon {
            Str(String),
            Char(char),
            BigInt(i128),
            Decimal(rust_decimal::Decimal),
            Identity,
        }
        let canon = match self.get_by_idx(v.as_heap_idx()) {
            Some(HeapObj::Str(s)) => Canon::Str(s.as_str().to_owned()),
            Some(HeapObj::Char(c)) => Canon::Char(*c),
            Some(HeapObj::BigInt(b)) => Canon::BigInt(*b),
            Some(HeapObj::Decimal(d)) => Canon::Decimal(**d),
            _ => Canon::Identity,
        };
        match canon {
            Canon::Str(s) => MapKey(self.alloc_str_interned(s)),
            Canon::Char(c) => MapKey(self.intern(Value::Char(c))),
            Canon::BigInt(b) => MapKey(self.intern(Value::BigInt(Box::new(b)))),
            Canon::Decimal(d) => MapKey(self.intern(Value::Decimal(Box::new(d)))),
            Canon::Identity => MapKey(v),
        }
    }

    pub(crate) fn alloc_range(&mut self, start: i64, end: i64, inclusive: bool) -> VmValue {
        let r = RangeData {
            start,
            end,
            inclusive,
            step: 1,
        };
        VmValue::from_heap_idx(self.alloc(HeapObj::Range(r)))
    }

    pub(crate) fn alloc_decimal(&mut self, d: rust_decimal::Decimal) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::Decimal(Box::new(d))))
    }

    pub(crate) fn alloc_vm_closure(&mut self, c: Rc<VmClosure>) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::VmClosure(c)))
    }

    pub fn alloc_module(&mut self, m: Rc<ModuleObj>) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::Module(m)))
    }

    pub(crate) fn alloc_frozen_module(&mut self, m: Arc<FrozenModuleObj>) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::FrozenModule(m)))
    }

    pub(crate) fn str_val(&self, nv: VmValue) -> Option<RuntimeString> {
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            let s = nv.sso_as_str(&mut buf);
            return Some(Rc::from(s));
        }
        if !nv.is_heap() {
            return None;
        }
        if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
            return Some(s.to_shared());
        }
        None
    }

    pub(crate) fn is_string(&self, nv: VmValue) -> bool {
        if nv.is_sso() {
            return true;
        }
        if nv.is_heap() {
            return matches!(self.get_by_idx(nv.as_heap_idx()), Some(HeapObj::Str(_)));
        }
        false
    }

    pub(crate) fn str_owned(&self, nv: VmValue) -> Option<String> {
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            let s = nv.sso_as_str(&mut buf);
            return Some(s.to_owned());
        }
        if nv.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
                return Some(s.to_string());
            }
        }
        None
    }

    pub(crate) fn str_repr_borrowed<'a>(&'a self, nv: VmValue) -> std::borrow::Cow<'a, str> {
        if nv.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
                return std::borrow::Cow::Borrowed(s.as_ref());
            }
        }
        std::borrow::Cow::Owned(self.str_repr(nv))
    }

    pub(crate) fn str_repr_into<W: std::fmt::Write>(&self, nv: VmValue, out: &mut W) {
        use crate::strbuf::{itoa, INT_MAX_DIGITS};
        if nv.is_null() {
            let _ = out.write_str("null");
        } else if nv.is_bool() {
            let _ = out.write_str(if nv.as_bool() { "true" } else { "false" });
        } else if nv.is_int() {
            let mut buf = [0u8; INT_MAX_DIGITS];
            let _ = out.write_str(itoa(nv.as_int(), &mut buf));
        } else if nv.is_f64() {
            let f = nv.as_f64();
            if f.fract() == 0.0 && f.abs() < 1e15 {
                let mut buf = [0u8; INT_MAX_DIGITS];
                let _ = out.write_str(itoa(f as i64, &mut buf));
            } else {
                let _ = write!(out, "{}", f);
            }
        } else if nv.is_sso() {
            let mut buf = [0u8; 5];
            let _ = out.write_str(nv.sso_as_str(&mut buf));
        } else if nv.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
                let _ = out.write_str(s.as_ref());
                return;
            }
            let _ = out.write_str(&self.str_repr(nv));
        } else {
            let _ = out.write_str(&self.str_repr(nv));
        }
    }

    pub(crate) fn str_repr(&self, nv: VmValue) -> String {
        if nv.is_null() {
            return "null".into();
        }
        if nv.is_bool() {
            return nv.as_bool().to_string();
        }
        if nv.is_int() {
            return nv.as_int().to_string();
        }
        if nv.is_f64() {
            let f = nv.as_f64();
            if f.fract() == 0.0 && f.abs() < 1e15 {
                return format!("{}", f as i64);
            }
            return format!("{}", f);
        }
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            return nv.sso_as_str(&mut buf).to_owned();
        }
        if nv.is_heap() {
            return match self.get_by_idx(nv.as_heap_idx()) {
                Some(HeapObj::Str(s)) => s.to_string(),
                Some(HeapObj::Char(c)) => c.to_string(),
                Some(HeapObj::Array(a)) => {
                    let parts: Vec<_> = (0..a.len())
                        .map(|i| self.str_repr(a.get_vm(i).unwrap()))
                        .collect();
                    format!("[{}]", parts.join(", "))
                }
                Some(HeapObj::Object(_)) => "[object Object]".into(),
                Some(HeapObj::VmClosure(nc)) => format!(
                    "[Function {}]",
                    nc.proto.name.as_deref().unwrap_or("<anon>")
                ),
                Some(HeapObj::NativeFn(name, _)) => format!("[NativeFn: {}]", name),
                Some(HeapObj::BoundMethod(method)) => match &method.target {
                    varn_types::value::BoundMethodTarget::Native { name, .. } => {
                        format!("[Function {}]", name)
                    }
                    varn_types::value::BoundMethodTarget::Vm { .. } => "[BoundMethod]".into(),
                },
                Some(HeapObj::Class(c)) => format!("[class {}]", c.name),
                Some(HeapObj::BigInt(n)) => n.to_string(),
                Some(HeapObj::Decimal(d)) => d.to_string(),
                _ => "[object]".into(),
            };
        }
        "null".into()
    }

    pub(crate) fn get_heap_idx(&self, val: VmValue) -> Option<u32> {
        if val.is_heap() {
            Some(val.as_heap_idx())
        } else {
            None
        }
    }

    pub(crate) fn value_heap_idx(&self, val: &varn_types::Value) -> Option<u32> {
        match val {
            varn_types::Value::Str(s) => self.string_interner.get(s).copied(),
            varn_types::Value::Array(a) => self.array_interner.get(a).copied(),
            varn_types::Value::Object(o) => self.object_interner.get(o).copied(),
            varn_types::Value::Map(m) => self.map_interner.get(m).copied(),
            varn_types::Value::Set(s) => self.set_interner.get(s).copied(),
            varn_types::Value::BigInt(b) => self.bigint_interner.get(b.as_ref()).copied(),
            varn_types::Value::Decimal(d) => self.decimal_interner.get(d.as_ref()).copied(),
            varn_types::Value::Char(c) => self.char_interner.get(c).copied(),
            varn_types::Value::Class(c) => self
                .identity_index
                .get(&(Rc::as_ptr(c) as usize))
                .copied(),
            varn_types::Value::Task(t) => self
                .identity_index
                .get(&(Rc::as_ptr(t) as usize))
                .copied(),
            varn_types::Value::TaskHandle(th) => {
                self.identity_index.get(&th.identity()).copied()
            }
            varn_types::Value::Generator(g) => self
                .identity_index
                .get(&(Rc::as_ptr(&g.0) as *const () as usize))
                .copied(),
            varn_types::Value::AsyncQueue(q) => self
                .identity_index
                .get(&(Rc::as_ptr(&q.0) as usize))
                .copied(),
            varn_types::Value::VmValue(payload) => {
                if let Some(wrapper) = payload.as_any().downcast_ref::<VmClosurePayload>() {
                    let closure_ptr = std::rc::Rc::as_ptr(&wrapper.0) as *const () as usize;
                    self.identity_index.get(&closure_ptr).copied()
                } else if let Some(vr) = payload.as_any().downcast_ref::<VmValueRef>() {
                    if vr.0.is_heap() {
                        Some(vr.0.as_heap_idx())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}
