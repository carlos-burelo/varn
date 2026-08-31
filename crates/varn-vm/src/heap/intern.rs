//! Interning and extraction: the bridge between `VmValue` (a packed word)
//! and `Value` (an owned Rust enum).
//!
//! `intern` is the direction that can allocate; `extract` is the direction
//! that can clone. Both are per-type, and the interner tables here are what
//! make equal literals share one heap slot.

use super::core::alloc_into;
use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::closure::{VmClosurePayload, VmValueRef};
use crate::error::{RuntimeError, VmResult};
use crate::nursery::pack_old_idx;
use crate::value::VmValue;
use std::collections::hash_map::Entry;
use std::rc::Rc;
use varn_types::{Value, VmArray};

impl HeapInner {
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
            Value::NativeFn(b) => VmValue::from_heap_idx(self.alloc(HeapObj::NativeFn(b.0, b.1))),
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
                    "[GC DEBUG] heap.extract failed on nv=0x{:016x}:0x{:016x} (is_heap={}, heap_idx={})",
                    nv.raw_tag(),
                    nv.raw_payload(),
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
                HeapObj::NativeFn(f, name) => Value::NativeFn(Box::new((*f, *name))),
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
                HeapObj::Spread(inner) => Value::Spread(Box::new(self.extract(*inner))),
                HeapObj::VmValue(payload) => Value::VmValue(payload.clone_payload()),
                HeapObj::Module(m) => Value::Module(m.clone()),
                HeapObj::Buffer(_) => Value::VmValue(Box::new(VmValueRef(nv))),
                HeapObj::FrozenModule(_) => Value::Null,
            });
        }
        Ok(Value::Null)
    }
}
