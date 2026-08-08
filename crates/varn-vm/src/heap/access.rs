//! Reading a heap slot back out.
//!
//! Indices are PACKED: the high bit distinguishes a nursery index from an
//! old-generation one, so every accessor has to unpack before it indexes.
//! `get_raw*` take an already-unpacked old index and skip that step.

use std::rc::Rc;
use crate::closure::{VmClosure, VmClosurePayload, VmValueRef};
use crate::nursery::{is_nursery_idx, old_idx_raw};
use crate::value::VmValue;
use super::obj::HeapObj;
use super::structs::HeapInner;

impl HeapInner {
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
