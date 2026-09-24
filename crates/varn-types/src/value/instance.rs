//! A user class instance: a class id and a payload laid out by the class's
//! `ClassLayout` (spec §47–§48).

use super::ClassObj;
use crate::class_layout::{ClassLayout, FieldLayout};
use crate::layout::{ScalarRepr, COMPACT_REF_NULL};
use crate::vm_value::VmValue;
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::ptr;
use std::rc::Rc;

/// Native static struct representation of a user class instance.
///
/// Unlike dynamic `ObjData`, an `InstanceData`:
/// - Has NO `Shape` pointer (classes have static layout).
/// - Has NO `overflow` store (no dynamic property additions).
/// - Has a compact 8-byte header (`class_id: u32`, `payload_size: u32`).
/// - Stores primitive fields packed at native offsets without 16-byte VmValue boxing.
#[repr(C, align(8))]
pub struct InstanceData<T: ?Sized = [UnsafeCell<u8>]> {
    pub class_id: u32,
    pub payload_size: u32,
    payload: T,
}

const INSTANCE_HEADER_WORDS: usize = 1;

impl InstanceData {
    /// Allocates an `InstanceData` on the heap with the specified layout.
    pub fn alloc(class: Rc<ClassObj>) -> Rc<InstanceData> {
        let layout = class.get_or_compute_layout();
        Self::alloc_with_layout(class.id, layout.payload_size)
    }

    /// Fast allocation of `InstanceData` when `class_id` and `payload_size` are already known.
    #[inline]
    pub fn alloc_with_layout(class_id: u32, payload_size: u32) -> Rc<InstanceData> {
        let payload_bytes = payload_size as usize;
        let payload_words = (payload_bytes + 7) / 8;
        let total_words = INSTANCE_HEADER_WORDS + payload_words;

        let backing: Rc<[MaybeUninit<u64>]> = Rc::new_uninit_slice(total_words);
        let base = Rc::into_raw(backing) as *const MaybeUninit<u64> as *mut UnsafeCell<u8>;
        let data = ptr::slice_from_raw_parts_mut(base, payload_bytes) as *mut InstanceData;

        unsafe {
            ptr::write(ptr::addr_of_mut!((*data).class_id), class_id);
            ptr::write(ptr::addr_of_mut!((*data).payload_size), payload_size);

            let payload_ptr = ptr::addr_of_mut!((*data).payload) as *mut u8;
            if payload_bytes > 0 {
                ptr::write_bytes(payload_ptr, 0, payload_bytes);
            }

            Rc::from_raw(data as *const InstanceData)
        }
    }

    #[inline(always)]
    pub fn raw_payload_ptr(&self) -> *mut u8 {
        self.payload.as_ptr() as *mut u8
    }

    // ── Field Read Methods ──────────────────────────────────────────────

    #[inline(always)]
    unsafe fn read_i64(&self, offset: usize) -> i64 {
        let ptr = self.raw_payload_ptr().add(offset) as *const i64;
        ptr.read()
    }

    #[inline(always)]
    unsafe fn read_f64(&self, offset: usize) -> f64 {
        let ptr = self.raw_payload_ptr().add(offset) as *const f64;
        ptr.read()
    }

    #[inline(always)]
    unsafe fn read_bool(&self, offset: usize) -> bool {
        let ptr = self.raw_payload_ptr().add(offset);
        *ptr != 0
    }



    #[inline(always)]
    unsafe fn read_u64(&self, offset: usize) -> u64 {
        let ptr = self.raw_payload_ptr().add(offset) as *const u64;
        ptr.read()
    }





    #[inline(always)]
    unsafe fn read_vm_value(&self, offset: usize) -> VmValue {
        let ptr = self.raw_payload_ptr().add(offset) as *const VmValue;
        ptr.read()
    }

    /// Resolves this instance's `ClassLayout` through the class registry.
    /// `InstanceData` itself only knows the header word (`class_id` /
    /// `payload_size`) needed for zero-copy allocation — the field table
    /// lives on `ClassObj` (one per class, not per instance) and is cached
    /// there (`get_or_compute_layout`), so this is a registry lookup plus a
    /// cached `Rc` clone, not a recomputation.
    #[inline]
    fn layout(&self) -> Option<Rc<ClassLayout>> {
        ClassObj::find_by_id(self.class_id).map(|c| c.get_or_compute_layout())
    }

    /// Number of declared fields — the one authority on how far a payload
    /// walk may go. Was `payload_size / 16` back when every field WAS
    /// exactly 16 bytes; now that fields pack at their real `FieldLayout`
    /// size, only the layout's own field count says how many there are.
    #[inline]
    pub fn slot_count(&self) -> usize {
        self.layout().map(|l| l.field_count()).unwrap_or(0)
    }

    #[inline]
    pub fn field_at(&self, slot: usize) -> Option<VmValue> {
        let layout = self.layout()?;
        let f = layout.get_field_by_index(slot)?;
        self.read_field(f)
    }

    /// Read a field by its BAKED compact `(offset, tag)` — no runtime layout
    /// lookup. Mirrors [`Self::read_field`] with the `FieldLayout` derived from
    /// the tag.
    #[inline]
    pub fn read_field_at(&self, offset: u32, tag: Option<varn_core::RuntimeKind>) -> Option<VmValue> {
        let f = FieldLayout::at(offset, tag);
        self.read_field(&f)
    }

    /// Write a field by its BAKED compact `(offset, tag)` — no runtime layout
    /// lookup.
    #[inline]
    pub fn write_field_at(
        &self,
        offset: u32,
        tag: Option<varn_core::RuntimeKind>,
        val: VmValue,
    ) -> Result<(), &'static str> {
        let f = FieldLayout::at(offset, tag);
        self.write_field(&f, val)
    }

    #[inline]
    pub fn set_field_at(&self, slot: usize, val: VmValue) -> bool {
        let Some(layout) = self.layout() else {
            return false;
        };
        let Some(f) = layout.get_field_by_index(slot) else {
            return false;
        };
        self.write_field(f, val).is_ok()
    }

    /// Reads one field by its representation (`TypeLayout`); a `Ref` slot
    /// decodes the `null` niche back to `null`, symmetric with the write.
    pub fn read_field(&self, f: &FieldLayout) -> Option<VmValue> {
        let offset = f.offset as usize;
        if offset + f.layout.size as usize > self.payload_size as usize {
            return None;
        }
        unsafe {
            Some(match f.layout.repr {
                ScalarRepr::Bool => VmValue::from_bool(self.read_bool(offset)),
                ScalarRepr::I64 => VmValue::from_int(self.read_i64(offset)),
                ScalarRepr::F64 => VmValue::from_f64(self.read_f64(offset)),
                ScalarRepr::Ref => {
                    let raw = self.read_u64(offset);
                    if raw == COMPACT_REF_NULL {
                        VmValue::null()
                    } else {
                        VmValue::from_heap_idx(raw as u32)
                    }
                }
                ScalarRepr::Boxed => self.read_vm_value(offset),
            })
        }
    }

    /// Writes one field at its own `FieldLayout`, converting the same way
    /// `varn_vm::frame_store`'s `Fpr`/`Ref` slot writes do: `int` widens into
    /// a `float` field the checker proved compatible, and `null` into a
    /// compact GC-ref field is the `null` niche, not an error — a `class`
    /// -typed field genuinely can be unset before the constructor assigns it
    /// (`tests/63-escape-analysis.vn`'s "unassigned field still reads null"
    /// pattern applies here exactly as it does to registers). Anything else
    /// wrong for the slot's class is a real type error, surfaced instead of
    /// silently reinterpreted.
    pub fn write_field(&self, f: &FieldLayout, val: VmValue) -> Result<(), &'static str> {
        let offset = f.offset as usize;
        if offset + f.layout.size as usize > self.payload_size as usize {
            return Err("field offset exceeds instance payload");
        }
        unsafe {
            match f.layout.repr {
                ScalarRepr::Bool => {
                    if !val.is_bool() {
                        return Err("cannot store non-bool in a bool field");
                    }
                    self.write_bool(offset, val.as_bool());
                }
                ScalarRepr::I64 => {
                    if !val.is_int() {
                        return Err("cannot store non-int in an int field");
                    }
                    self.write_i64(offset, val.as_int());
                }
                ScalarRepr::F64 => {
                    // Symmetric with `frame_store`'s `Fpr`: `int` widens,
                    // `null` (a NaN result — `VmValue::from_f64` already
                    // folds NaN to `null`) round-trips through a real NaN
                    // bit pattern instead of being rejected.
                    if val.is_f64() {
                        self.write_f64(offset, val.as_f64());
                    } else if val.is_int() {
                        self.write_f64(offset, val.as_int() as f64);
                    } else if val.is_null() {
                        self.write_f64(offset, f64::NAN);
                    } else {
                        return Err("cannot store non-numeric in a float field");
                    }
                }
                ScalarRepr::Ref => {
                    if val.is_null() {
                        self.write_u64(offset, COMPACT_REF_NULL);
                    } else if val.is_heap() {
                        self.write_u64(offset, val.as_heap_idx() as u64);
                    } else {
                        return Err("cannot store non-reference in a reference field");
                    }
                }
                ScalarRepr::Boxed => self.write_vm_value(offset, val),
            }
        }
        Ok(())
    }

    // ── Field Write Methods ─────────────────────────────────────────────

    #[inline(always)]
    unsafe fn write_i64(&self, offset: usize, val: i64) {
        let ptr = self.raw_payload_ptr().add(offset) as *mut i64;
        ptr.write(val);
    }

    #[inline(always)]
    unsafe fn write_f64(&self, offset: usize, val: f64) {
        let ptr = self.raw_payload_ptr().add(offset) as *mut f64;
        ptr.write(val);
    }

    #[inline(always)]
    unsafe fn write_bool(&self, offset: usize, val: bool) {
        let ptr = self.raw_payload_ptr().add(offset);
        *ptr = val as u8;
    }



    #[inline(always)]
    unsafe fn write_u64(&self, offset: usize, val: u64) {
        let ptr = self.raw_payload_ptr().add(offset) as *mut u64;
        ptr.write(val);
    }

    #[inline(always)]
    unsafe fn write_vm_value(&self, offset: usize, val: VmValue) {
        let ptr = self.raw_payload_ptr().add(offset) as *mut VmValue;
        ptr.write(val);
    }
}

/// Reference-counted wrapper around [`InstanceData`].
#[derive(Clone)]
pub struct InstanceRef(pub Rc<InstanceData>);

impl InstanceRef {
    #[inline]
    pub fn alloc(class: Rc<ClassObj>) -> Self {
        Self(InstanceData::alloc(class))
    }

    #[inline]
    pub fn alloc_with_layout(class_id: u32, payload_size: u32) -> Self {
        Self(InstanceData::alloc_with_layout(class_id, payload_size))
    }

    #[inline(always)]
    pub fn read(&self) -> &InstanceData {
        &self.0
    }
}

impl std::ops::Deref for InstanceRef {
    type Target = InstanceData;
    #[inline(always)]
    fn deref(&self) -> &InstanceData {
        &self.0
    }
}

impl std::fmt::Debug for InstanceData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstanceData")
            .field("class_id", &self.class_id)
            .field("payload_size", &self.payload_size)
            .finish()
    }
}

impl std::fmt::Debug for InstanceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "InstanceRef(class_id={}, size={})",
            self.0.class_id, self.0.payload_size
        )
    }
}

impl PartialEq for InstanceRef {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for InstanceRef {}

impl std::hash::Hash for InstanceRef {
    #[inline(always)]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}
