use crate::error::{RuntimeError, VmResult};
use crate::frame::{VmClosure, VmClosurePayload, VmValueRef};
use crate::gc::GcCollector;
use crate::nursery::{
    is_nursery_idx, is_old_idx, old_idx_raw, pack_old_idx, Nursery, OLD_GEN_FLAG,
};
use crate::value::VmValue;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use varn_types::{
    generator::{AsyncQueue, GeneratorObj},
    value::{
        ArrayRef, EnumVariantData, MapRef, ModuleObj, ObjRef, RangeData, RuntimeSymbol, SetRef,
    },
    AsyncTask, ClassObj, LazyTask, NativeCtx, NativeFn, ObjData, ResourceStore, RuntimeString,
    Value, VmArray,
};

#[derive(Debug, Clone)]
pub enum HeapObj {
    Str(RuntimeString),
    Array(VmArray),
    Object(ObjRef),

    Module(Rc<ModuleObj>),
    VmClosure(Rc<VmClosure>),
    Class(Rc<ClassObj>),
    NativeFn(&'static str, NativeFn),
    BoundMethod(Box<varn_types::value::BoundMethod>),
    Map(MapRef),
    Set(SetRef),
    Task(Rc<LazyTask>),
    TaskHandle(AsyncTask),
    Range(RangeData),
    Symbol(RuntimeSymbol),
    EnumVariant(Box<EnumVariantData>),
    BigInt(i128),
    Decimal(Box<rust_decimal::Decimal>),
    Char(char),
    Generator(GeneratorObj),
    AsyncQueue(AsyncQueue),
    Spread(VmValue),
}

#[derive(Clone)]
pub struct HeapInner {
    pub alloc_count: u64,
    pub intrinsic_classes: HashMap<String, Rc<ClassObj>>,
    pub gc_collections: u64,
    pub gc_total_freed: u64,
    pub gc_alloc_since_collect: u64,
    pub nursery: Nursery,
    free: Vec<u32>,
    objects: Vec<Option<HeapObj>>,
    string_interner: HashMap<RuntimeString, u32>,
    symbol_interner: HashMap<RuntimeSymbol, u32>,
    array_interner: HashMap<ArrayRef, u32>,
    object_interner: HashMap<ObjRef, u32>,
    map_interner: HashMap<MapRef, u32>,
    set_interner: HashMap<SetRef, u32>,
    bigint_interner: HashMap<i128, u32>,
    decimal_interner: HashMap<rust_decimal::Decimal, u32>,
    char_interner: HashMap<char, u32>,
    gc_collector: Option<GcCollector>,
}

#[inline]
fn alloc_into(
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
    pub fn new() -> Self {
        Self {
            objects: Vec::with_capacity(4096),
            free: Vec::new(),
            alloc_count: 0,
            intrinsic_classes: HashMap::new(),
            string_interner: HashMap::new(),
            symbol_interner: HashMap::new(),
            array_interner: HashMap::new(),
            object_interner: HashMap::new(),
            map_interner: HashMap::new(),
            set_interner: HashMap::new(),
            bigint_interner: HashMap::new(),
            decimal_interner: HashMap::new(),
            char_interner: HashMap::new(),
            gc_collector: Some(GcCollector::new(4096)),
            gc_collections: 0,
            gc_total_freed: 0,
            gc_alloc_since_collect: 0,
            nursery: Nursery::new(),
        }
    }

    pub fn set_intrinsic_class(&mut self, name: &str, cls: Rc<ClassObj>) {
        self.intrinsic_classes.insert(name.to_string(), cls);
    }

    pub fn get_intrinsic_class(&self, name: &str) -> Option<Rc<ClassObj>> {
        self.intrinsic_classes.get(name).cloned()
    }

    #[inline]
    pub fn alloc_native_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::NativeFn(name, f)))
    }

    pub fn alloc(&mut self, obj: HeapObj) -> u32 {
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
            | HeapObj::Decimal(_) => {
                if let Some(ni) = self.nursery.try_alloc(obj.clone()) {
                    return ni;
                }
            }
            _ => {}
        }
        pack_old_idx(alloc_into(
            &mut self.objects,
            &mut self.free,
            &mut self.alloc_count,
            &mut self.gc_alloc_since_collect,
            obj,
        ))
    }

    pub fn alloc_raw(&mut self, obj: HeapObj) -> u32 {
        self.alloc_count += 1;
        if let Some(idx) = self.free.pop() {
            self.objects[idx as usize] = Some(obj);
            idx
        } else {
            let idx = self.objects.len() as u32;
            self.objects.push(Some(obj));
            idx
        }
    }

    #[inline(always)]
    pub fn needs_minor_gc(&self) -> bool {
        self.nursery.is_full()
    }

    #[inline(always)]
    pub fn write_barrier(&mut self, parent_packed_idx: u32, new_val: VmValue) {
        if is_old_idx(parent_packed_idx)
            && new_val.is_heap()
            && is_nursery_idx(new_val.as_heap_idx())
        {
            self.nursery.remember(parent_packed_idx);
        }
    }

    #[inline(always)]
    pub fn get_by_idx(&self, idx: u32) -> Option<&HeapObj> {
        if is_nursery_idx(idx) {
            self.nursery.get(idx)
        } else {
            self.objects.get(old_idx_raw(idx) as usize)?.as_ref()
        }
    }

    #[inline(always)]
    pub fn get_by_idx_mut(&mut self, idx: u32) -> Option<&mut HeapObj> {
        if is_nursery_idx(idx) {
            self.nursery.get_mut(idx)
        } else {
            self.objects.get_mut(old_idx_raw(idx) as usize)?.as_mut()
        }
    }

    pub fn minor_gc(&mut self, stack: &mut [VmValue], extra_packed: &[u32]) {
        let mut nursery = std::mem::take(&mut self.nursery);
        nursery.collect(self, stack, extra_packed);
        self.nursery = nursery;
    }

    #[inline(always)]
    pub fn get(&self, idx: u32) -> Option<&HeapObj> {
        self.get_by_idx(idx)
    }

    #[inline(always)]
    pub fn get_or_panic(&self, idx: u32) -> &HeapObj {
        self.get_by_idx(idx).expect("invalid heap index")
    }

    #[inline(always)]
    pub fn get_mut(&mut self, idx: u32) -> Option<&mut HeapObj> {
        self.get_by_idx_mut(idx)
    }

    #[inline(always)]
    pub unsafe fn get_mut_unchecked(&mut self, idx: u32) -> &mut HeapObj {
        self.get_by_idx_mut(idx).expect("invalid heap index")
    }

    #[inline(always)]
    pub fn get_raw(&self, raw_old_idx: u32) -> Option<&HeapObj> {
        self.objects.get(raw_old_idx as usize)?.as_ref()
    }

    #[inline(always)]
    pub fn get_raw_mut(&mut self, raw_old_idx: u32) -> Option<&mut HeapObj> {
        self.objects.get_mut(raw_old_idx as usize)?.as_mut()
    }

    pub fn intern(&mut self, val: Value) -> VmValue {
        match val {
            Value::Null => VmValue::null(),
            Value::Bool(b) => VmValue::from_bool(b),
            Value::Int(n) if n >= -(1i64 << 47) && n <= (1i64 << 47) - 1 => VmValue::from_int(n),
            Value::Int(n) => VmValue::from_f64(n as f64),
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
                drop(guard);
                let va = VmArray::new(vm_items);
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
                    VmValue::null()
                }
            }
            Value::Module(m) => VmValue::from_heap_idx(self.alloc(HeapObj::Module(m))),
        }
    }

    pub fn extract(&self, nv: VmValue) -> Value {
        match self.extract_val(nv) {
            Ok(v) => v,
            Err(e) => panic!("heap.extract: dangling or corrupted heap reference: {e}"),
        }
    }

    pub fn extract_val(&self, nv: VmValue) -> VmResult<Value> {
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
                HeapObj::Str(s) => Value::Str(s.clone()),
                HeapObj::Array(a) => {
                    let guard = a.borrow();
                    let val_items: Vec<Value> = guard.iter().map(|&nv| self.extract(nv)).collect();
                    drop(guard);
                    Value::Array(varn_types::value::ArrayRef::new(val_items))
                }
                HeapObj::Object(o) => Value::Object(o.clone()),
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

                HeapObj::Module(m) => Value::Module(m.clone()),
            });
        }
        Ok(Value::Null)
    }

    pub fn alloc_str(&mut self, s: impl AsRef<str>) -> VmValue {
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

        let idx = if let Some(ni) = self.nursery.try_alloc(HeapObj::Str(rs.clone())) {
            ni
        } else {
            let oi = alloc_into(
                &mut self.objects,
                &mut self.free,
                &mut self.alloc_count,
                &mut self.gc_alloc_since_collect,
                HeapObj::Str(rs.clone()),
            );
            let packed = pack_old_idx(oi);
            self.string_interner.insert(rs, packed);
            packed
        };
        VmValue::from_heap_idx(idx)
    }

    pub fn alloc_str_interned(&mut self, s: impl AsRef<str>) -> VmValue {
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
        let oi = alloc_into(
            &mut self.objects,
            &mut self.free,
            &mut self.alloc_count,
            &mut self.gc_alloc_since_collect,
            HeapObj::Str(rs.clone()),
        );
        let packed = pack_old_idx(oi);
        self.string_interner.insert(rs, packed);
        VmValue::from_heap_idx(packed)
    }

    pub fn alloc_symbol(&mut self, s: RuntimeSymbol) -> VmValue {
        let packed = match self.symbol_interner.entry(s) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => {
                let packed = pack_old_idx(alloc_into(
                    &mut self.objects,
                    &mut self.free,
                    &mut self.alloc_count,
                    &mut self.gc_alloc_since_collect,
                    HeapObj::Symbol(e.key().clone()),
                ));
                *e.insert(packed)
            }
        };
        VmValue::from_heap_idx(packed)
    }

    pub fn alloc_array_vm(&mut self, items: Vec<VmValue>) -> VmValue {
        let va = VmArray::new(items);
        VmValue::from_heap_idx(self.alloc(HeapObj::Array(va)))
    }

    pub fn alloc_array(&mut self, items: Vec<Value>) -> VmValue {
        let vm_items: Vec<VmValue> = items.into_iter().map(|v| self.intern(v)).collect();
        self.alloc_array_vm(vm_items)
    }

    pub fn alloc_object(&mut self) -> VmValue {
        let oref = ObjRef::new(ObjData::new());
        VmValue::from_heap_idx(self.alloc(HeapObj::Object(oref)))
    }

    pub fn alloc_range(&mut self, start: i64, end: i64, inclusive: bool) -> VmValue {
        let r = RangeData {
            start,
            end,
            inclusive,
            step: 1,
        };
        VmValue::from_heap_idx(self.alloc(HeapObj::Range(r)))
    }

    pub fn alloc_decimal(&mut self, d: rust_decimal::Decimal) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::Decimal(Box::new(d))))
    }

    pub fn alloc_vm_closure(&mut self, c: Rc<VmClosure>) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::VmClosure(c)))
    }

    pub fn alloc_module(&mut self, m: Rc<ModuleObj>) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::Module(m)))
    }

    pub fn str_val(&self, nv: VmValue) -> Option<RuntimeString> {
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            let s = nv.sso_as_str(&mut buf);
            return Some(Rc::from(s));
        }
        if !nv.is_heap() {
            return None;
        }
        if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
            return Some(s.clone());
        }
        None
    }

    pub fn is_string(&self, nv: VmValue) -> bool {
        if nv.is_sso() {
            return true;
        }
        if nv.is_heap() {
            return matches!(self.get_by_idx(nv.as_heap_idx()), Some(HeapObj::Str(_)));
        }
        false
    }

    pub fn str_owned(&self, nv: VmValue) -> Option<String> {
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

    pub fn str_repr_borrowed<'a>(&'a self, nv: VmValue) -> std::borrow::Cow<'a, str> {
        if nv.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
                return std::borrow::Cow::Borrowed(s.as_ref());
            }
        }
        std::borrow::Cow::Owned(self.str_repr(nv))
    }

    pub fn str_repr(&self, nv: VmValue) -> String {
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
                    let g = a.borrow();
                    let parts: Vec<_> = g.iter().map(|&v| self.str_repr(v)).collect();
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

    pub fn objects_len(&self) -> u32 {
        self.objects.len() as u32
    }

    pub fn get_heap_idx(&self, val: VmValue) -> Option<u32> {
        if val.is_heap() {
            Some(val.as_heap_idx())
        } else {
            None
        }
    }

    pub fn free_list_mut(&mut self) -> &mut Vec<u32> {
        &mut self.free
    }

    pub fn should_trigger_gc(&self) -> bool {
        let used = self.objects.len() - self.free.len();

        self.free.len() * 5 < self.objects.len() || used > 10000
    }

    #[inline(always)]
    pub fn needs_gc(&self) -> bool {
        self.gc_alloc_since_collect >= 4096
    }

    pub fn compact_interners(&mut self) {
        self.string_interner.retain(|_, &mut packed| {
            let raw = old_idx_raw(packed);
            self.objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
        });

        let check = |packed: u32, objects: &Vec<Option<HeapObj>>| -> bool {
            let raw = old_idx_raw(packed);
            objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
        };

        self.symbol_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.char_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.bigint_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.decimal_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.array_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.object_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.map_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.set_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
    }

    pub fn objects(&self) -> &Vec<Option<HeapObj>> {
        &self.objects
    }

    pub fn value_heap_idx(&self, val: &varn_types::Value) -> Option<u32> {
        match val {
            varn_types::Value::Str(s) => self.string_interner.get(s).copied(),
            varn_types::Value::Array(a) => self.array_interner.get(a).copied(),
            varn_types::Value::Object(o) => self.object_interner.get(o).copied(),
            varn_types::Value::Map(m) => self.map_interner.get(m).copied(),
            varn_types::Value::Set(s) => self.set_interner.get(s).copied(),
            varn_types::Value::BigInt(b) => self.bigint_interner.get(b.as_ref()).copied(),
            varn_types::Value::Decimal(d) => self.decimal_interner.get(d.as_ref()).copied(),
            varn_types::Value::Char(c) => self.char_interner.get(c).copied(),
            varn_types::Value::Class(c) => {
                for (idx, obj) in self.objects.iter().enumerate() {
                    if let Some(HeapObj::Class(hc)) = obj {
                        if Rc::ptr_eq(c, hc) {
                            return Some(idx as u32);
                        }
                    }
                }
                None
            }
            varn_types::Value::Task(t) => {
                for (idx, obj) in self.objects.iter().enumerate() {
                    if let Some(HeapObj::Task(ht)) = obj {
                        if Rc::ptr_eq(t, ht) {
                            return Some(idx as u32);
                        }
                    }
                }
                None
            }
            varn_types::Value::TaskHandle(th) => {
                for (idx, obj) in self.objects.iter().enumerate() {
                    if let Some(HeapObj::TaskHandle(hth)) = obj {
                        if th == hth {
                            return Some(idx as u32);
                        }
                    }
                }
                None
            }
            varn_types::Value::Generator(g) => {
                for (idx, obj) in self.objects.iter().enumerate() {
                    if let Some(HeapObj::Generator(hg)) = obj {
                        if g == hg {
                            return Some(idx as u32);
                        }
                    }
                }
                None
            }
            varn_types::Value::AsyncQueue(q) => {
                for (idx, obj) in self.objects.iter().enumerate() {
                    if let Some(HeapObj::AsyncQueue(hq)) = obj {
                        if Rc::ptr_eq(&q.0, &hq.0) {
                            return Some(idx as u32);
                        }
                    }
                }
                None
            }
            varn_types::Value::VmValue(payload) => {
                if let Some(wrapper) = payload.as_any().downcast_ref::<VmClosurePayload>() {
                    let closure_ptr = std::rc::Rc::as_ptr(&wrapper.0) as *const () as usize;
                    for (idx, obj) in self.objects.iter().enumerate() {
                        if let Some(HeapObj::VmClosure(hc)) = obj {
                            if std::rc::Rc::as_ptr(hc) as *const () as usize == closure_ptr {
                                return Some(idx as u32);
                            }
                        }
                    }
                } else if let Some(vr) = payload.as_any().downcast_ref::<VmValueRef>() {
                    if vr.0.is_heap() {
                        return Some(vr.0.as_heap_idx());
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub fn objects_mut(&mut self) -> &mut Vec<Option<HeapObj>> {
        &mut self.objects
    }

    pub fn collect(&mut self, roots: &[u32]) -> Result<usize, crate::gc::GcError> {
        if let Some(mut collector) = self.gc_collector.take() {
            let freed = collector.collect(self, roots)?;
            self.gc_collector = Some(collector);
            self.compact_interners();
            self.gc_collections += 1;
            self.gc_total_freed += freed as u64;
            self.gc_alloc_since_collect = 0;
            Ok(freed)
        } else {
            Ok(0)
        }
    }

    pub fn free_count(&self) -> usize {
        self.free.len()
    }

    pub fn live_count(&self) -> usize {
        self.objects.len().saturating_sub(self.free.len())
    }

    pub fn enable_gc(&mut self) {
        if self.gc_collector.is_none() {
            self.gc_collector = Some(GcCollector::new(self.objects.len() as u32));
        }
    }

    pub fn disable_gc(&mut self) {
        self.gc_collector = None;
    }

    pub fn gc_enabled(&self) -> bool {
        self.gc_collector.is_some()
    }

    pub fn native_module_get(&self, obj: VmValue, key: &str) -> Option<VmValue> {
        let idx = obj.as_heap_idx();
        if let Some(HeapObj::Module(m)) = self.get(idx) {
            let &slot = m.export_map.get(key)?;
            return m.get_slot(slot);
        }
        None
    }
}

#[derive(Clone)]
pub struct Heap {
    inner: Rc<std::cell::UnsafeCell<HeapInner>>,
}

impl Heap {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(std::cell::UnsafeCell::new(HeapInner::new())),
        }
    }

    pub fn deep_clone(&self) -> Self {
        let inner_clone = unsafe { (*self.inner.get()).clone() };
        Self {
            inner: Rc::new(std::cell::UnsafeCell::new(inner_clone)),
        }
    }

    #[inline(always)]
    pub unsafe fn inner_mut(&self) -> &mut HeapInner {
        &mut *self.inner.get()
    }
}

impl std::ops::Deref for Heap {
    type Target = HeapInner;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner.get() }
    }
}

impl std::ops::DerefMut for Heap {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.inner.get() }
    }
}

impl std::fmt::Debug for Heap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Heap {{ alloc_count: {} }}", self.alloc_count)
    }
}

impl NativeCtx for Heap {
    fn alloc_str(&mut self, s: &str) -> VmValue {
        self.deref_mut().alloc_str(s)
    }

    fn alloc_str_owned(&mut self, s: String) -> VmValue {
        self.deref_mut().alloc_str(&s)
    }

    fn alloc_array(&mut self, items: Vec<VmValue>) -> VmValue {
        self.alloc_array_vm(items)
    }

    fn alloc_object(&mut self) -> VmValue {
        self.deref_mut().alloc_object()
    }

    fn alloc_range(&mut self, start: i64, end: i64, inclusive: bool) -> VmValue {
        self.deref_mut().alloc_range(start, end, inclusive)
    }

    fn alloc_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue {
        self.alloc_native_fn(f, name)
    }

    fn alloc_class(&mut self, class: Rc<ClassObj>) -> VmValue {
        self.intern(Value::Class(class))
    }

    fn is_string(&self, v: VmValue) -> bool {
        self.deref().is_string(v)
    }

    fn is_array(&self, v: VmValue) -> bool {
        v.is_heap() && matches!(self.get_by_idx(v.as_heap_idx()), Some(HeapObj::Array(_)))
    }

    fn str_repr(&self, v: VmValue) -> String {
        self.deref().str_repr(v)
    }

    fn str_repr_borrowed<'a>(&'a self, v: VmValue) -> std::borrow::Cow<'a, str> {
        self.deref().str_repr_borrowed(v)
    }

    fn str_owned(&self, v: VmValue) -> Option<String> {
        self.deref().str_owned(v)
    }

    fn array_len(&self, arr: VmValue) -> usize {
        if arr.is_heap() {
            if let Some(HeapObj::Array(a)) = self.get_by_idx(arr.as_heap_idx()) {
                return a.borrow().len();
            }
        }
        0
    }

    fn array_get(&self, arr: VmValue, idx: usize) -> Option<VmValue> {
        if arr.is_heap() {
            if let Some(HeapObj::Array(a)) = self.get_by_idx(arr.as_heap_idx()) {
                return a.borrow().get(idx).copied();
            }
        }
        None
    }

    fn array_set(&mut self, arr: VmValue, idx: usize, val: VmValue) {
        if arr.is_heap() {
            if let Some(HeapObj::Array(a)) = self.get_by_idx(arr.as_heap_idx()) {
                let mut g = a.borrow_mut();
                if idx < g.len() {
                    g[idx] = val;
                }
            }
        }
    }

    fn array_push(&mut self, arr: VmValue, val: VmValue) {
        if arr.is_heap() {
            if let Some(HeapObj::Array(a)) = self.get_by_idx(arr.as_heap_idx()) {
                a.borrow_mut().push(val);
            }
        }
    }

    fn array_pop(&mut self, arr: VmValue) -> Option<VmValue> {
        if arr.is_heap() {
            if let Some(HeapObj::Array(a)) = self.get_by_idx(arr.as_heap_idx()) {
                return a.borrow_mut().pop();
            }
        }
        None
    }

    fn array_for_each(&self, arr: VmValue, f: &mut dyn FnMut(VmValue, usize)) {
        if arr.is_heap() {
            if let Some(HeapObj::Array(a)) = self.get_by_idx(arr.as_heap_idx()) {
                let g = a.borrow();
                for (i, &v) in g.iter().enumerate() {
                    f(v, i);
                }
            }
        }
    }

    fn get_field(&self, obj: VmValue, key: &str) -> Option<VmValue> {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o)) = self.get_by_idx(obj.as_heap_idx()) {
                return o.borrow().get_field_nv(key);
            }
            if let Some(HeapObj::Module(m)) = self.get_by_idx(obj.as_heap_idx()) {
                let slot = m.export_map.get(key).copied()?;
                return m.get_slot(slot);
            }
        }
        None
    }

    fn set_field(&mut self, obj: VmValue, key: &str, val: VmValue) {
        if obj.is_heap() {
            let raw_idx = obj.as_heap_idx();
            if let Some(HeapObj::Object(o)) = self.get_by_idx(raw_idx) {
                o.borrow_mut().set_field_nv(Rc::from(key), val);
            } else if let Some(HeapObj::Module(m)) = self.get_by_idx_mut(raw_idx) {
                if let Some(s) = m.export_map.get(key).copied() {
                    Rc::make_mut(m).set_slot(s, val);
                } else {
                    let m = Rc::make_mut(m);
                    let slot = m.exports.len();
                    m.exports.push(val);
                    m.export_map.insert(Rc::from(key), slot);
                }
            }
        }
    }

    fn finalize(&mut self, obj: VmValue) -> VmValue {
        obj
    }

    fn call_vm(&mut self, _callee: VmValue, _args: &[VmValue]) -> Result<VmValue, String> {
        Err("call_vm unavailable on bare Heap (use ExecCtx)".into())
    }

    fn spawn_vm(&mut self, _callee: VmValue, _args: &[VmValue]) -> Result<VmValue, String> {
        Err("spawn_vm unavailable on bare Heap (use ExecCtx)".into())
    }

    fn set_timer(
        &mut self,
        _ms: u64,
        _repeat: bool,
        _callee: VmValue,
        _args: &[VmValue],
    ) -> Result<usize, String> {
        Err("set_timer unavailable on bare Heap".into())
    }

    fn clear_timer(&mut self, _id: usize) -> Result<(), String> {
        Ok(())
    }

    fn suspend_timer(&mut self, _ms: u64) -> VmValue {
        VmValue::null()
    }

    fn resources(&mut self) -> &mut ResourceStore {
        panic!("resources() unavailable on bare Heap")
    }

    fn extract(&self, v: VmValue) -> Value {
        self.deref().extract(v)
    }

    fn intern(&mut self, v: Value) -> VmValue {
        self.deref_mut().intern(v)
    }

    fn call_static(&mut self, f: NativeFn) -> VmValue {
        varn_types::call_static_with(self, f)
    }

    fn get_class(&self, name: &str) -> Option<std::rc::Rc<ClassObj>> {
        self.get_intrinsic_class(name)
    }

    fn register_class(&mut self, name: &str, cls: std::rc::Rc<ClassObj>) {
        self.set_intrinsic_class(name, cls);
    }
}
