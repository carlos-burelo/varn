//! The allocation core: how an object gets a heap slot.
//!
//! Every `alloc_*` in the sibling modules funnels through `alloc`, which is
//! also where the nursery is offered the object first. `alloc_raw` skips the
//! nursery for objects that must be born old.

use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::nursery::pack_old_idx;
use crate::value::VmValue;
use varn_types::NativeFn;

#[inline]
pub(crate) fn alloc_into(
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
        VmValue::from_heap_idx(self.alloc(HeapObj::NativeFn(f, name)))
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
            | HeapObj::Instance(_)
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
        // Reaching here means the nursery was full, so this object is BORN in
        // the old generation. If it already holds nursery references — which a
        // bulk build such as `JSON.parse` does constantly — nothing else will
        // ever record that: the write barrier only fires on later writes, and
        // `scan_roots` is for kinds whose children no barrier covers.
        //
        // Without this, a `JSON.parse` of 50 000 objects left its result array
        // in the old generation pointing at nursery elements; the next minor
        // collection evacuated them without updating the array, and the
        // following read panicked with "dangling or corrupted heap reference".
        let remember = crate::nursery::Nursery::holds_nursery_ref(&obj);
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
        if remember {
            self.nursery.remember(pack_old_idx(raw));
        }
        if let Some(key) = identity {
            self.identity_index.insert(key, pack_old_idx(raw));
        }
        pack_old_idx(raw)
    }

    pub(crate) fn alloc_raw(&mut self, obj: HeapObj) -> u32 {
        self.alloc_count += 1;
        self.gc_alloc_since_collect += 1;
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
}
