use super::{body, header, CellSpace, SlotState, HEADER_BYTES, INSTANCE_DATA_OFF};
use crate::heap::obj::HeapObj;
use std::rc::Rc;
use varn_types::value::{InstanceData, InstanceRef, ObjData, Shape};
use varn_types::HeapRef;

impl CellSpace {
    #[inline]
    pub(crate) fn alloc(&mut self, obj: HeapObj, state: SlotState) -> HeapRef {
        let r = self.take_cell(std::mem::size_of::<HeapObj>(), 0, state);
        Self::place(r, obj);
        r
    }

    /// An instance whose payload lives in the same cell, right after the
    /// `HeapObj` that names it: one allocation, no separate body.
    #[inline]
    pub(crate) fn alloc_instance(
        &mut self,
        class_id: u32,
        payload_size: u32,
        state: SlotState,
    ) -> (HeapRef, InstanceRef) {
        let tail = INSTANCE_DATA_OFF - HEADER_BYTES;
        let r = self.take_cell(tail + InstanceData::bytes_for(payload_size), 0, state);
        let data = (r.addr() as usize + INSTANCE_DATA_OFF) as *mut u8;
        let inst = unsafe { InstanceData::init_at(data, class_id, payload_size) };
        Self::place(r, HeapObj::Instance(inst));
        (r, inst)
    }

    /// A property object (`record` for a record) whose fields live in the
    /// same cell, right after the `HeapObj` that names it.
    #[inline]
    pub(crate) fn alloc_object(
        &mut self,
        record: bool,
        shape: Rc<Shape>,
        n: usize,
        values: &[varn_types::VmValue],
        state: SlotState,
    ) -> HeapRef {
        let tail = std::mem::size_of::<HeapObj>();
        let r = self.take_cell(tail + ObjData::bytes_for(n), 0, state);
        let obj = unsafe { ObjData::init_at(body::<u8>(r).add(tail), shape, n, values) };
        Self::place(
            r,
            if record {
                HeapObj::Record(obj)
            } else {
                HeapObj::Object(obj)
            },
        );
        r
    }

    /// An array (`tuple` for a tuple) whose repr lives in the same cell,
    /// right after the `HeapObj` that names it.
    #[inline]
    pub(crate) fn alloc_array(
        &mut self,
        tuple: bool,
        repr: varn_types::vm_value::ArrayRepr,
        state: SlotState,
    ) -> HeapRef {
        let tail = std::mem::size_of::<HeapObj>();
        let bytes = std::mem::size_of::<varn_types::vm_value::ArrayRepr>();
        let r = self.take_cell(tail + bytes, 0, state);
        let arr = unsafe { varn_types::VmArray::init_at(body::<u8>(r).add(tail).cast(), repr) };
        Self::place(
            r,
            if tuple {
                HeapObj::Tuple(arr)
            } else {
                HeapObj::Array(arr)
            },
        );
        r
    }

    /// Drops the object in `r` and what its cell owns beyond it.
    pub(super) unsafe fn drop_object(r: HeapRef) {
        let obj = body::<HeapObj>(r);
        match &*obj {
            HeapObj::Object(o) | HeapObj::Record(o) => ObjData::drop_at(*o),
            HeapObj::Array(a) | HeapObj::Tuple(a) => a.drop_at(),
            _ => {}
        }
        std::ptr::drop_in_place(obj);
    }

    #[inline(always)]
    fn place(r: HeapRef, obj: HeapObj) {
        header(r).kind = unsafe { *(&obj as *const HeapObj as *const u8) };
        unsafe { std::ptr::write(body::<HeapObj>(r), obj) };
    }
}
