use super::{body, header, CellSpace, SlotState};
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

    #[inline]
    pub(crate) fn alloc_instance(
        &mut self,
        class_id: u32,
        payload_size: u32,
        state: SlotState,
    ) -> (HeapRef, InstanceRef) {
        use varn_types::cell::CELL_KIND_INSTANCE;
        let r = self.take_cell(
            varn_types::cell::instance_colocated_body_bytes(payload_size),
            CELL_KIND_INSTANCE,
            state,
        );
        let data = (r.addr() as usize + varn_types::cell::INST_CELL_DATA_OFF) as *mut u8;
        let inst = unsafe { InstanceData::init_at(data, class_id, payload_size) };
        (r, inst)
    }

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
