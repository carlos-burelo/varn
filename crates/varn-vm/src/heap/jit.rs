use super::obj::HeapObj;
use super::structs::{Heap, HeapInner};
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::value::ObjRef;

impl Heap {
    pub(crate) fn nursery_len_byte_offset_from_rcbox() -> usize {
        2 * std::mem::size_of::<usize>()
            + std::mem::offset_of!(HeapInner, nursery)
            + crate::nursery::Nursery::objects_len_byte_offset()
    }

    pub(crate) fn rcbox_ptr_for_validation(&self) -> *const u8 {
        (Rc::as_ptr(&self.inner) as *const u8).wrapping_sub(2 * std::mem::size_of::<usize>())
    }

    pub(crate) fn jit_array_layout() -> varn_jit::JitArrayLayout {
        fn vec_word_offsets<T>(v: &Vec<T>) -> (usize, usize) {
            assert_eq!(v.len(), 3);
            assert_eq!(v.capacity(), 7);
            let words: [usize; 3] = unsafe { std::mem::transmute_copy(v) };
            let ptr = v.as_ptr() as usize;
            let mut ptr_off = usize::MAX;
            let mut len_off = usize::MAX;
            for (i, w) in words.iter().enumerate() {
                if *w == ptr {
                    ptr_off = i * 8;
                } else if *w == 3 {
                    len_off = i * 8;
                }
            }
            assert!(
                ptr_off != usize::MAX && len_off != usize::MAX,
                "Vec layout probe failed"
            );
            (ptr_off, len_off)
        }

        let mut slots_probe: Vec<Option<HeapObj>> = Vec::with_capacity(7);
        for _ in 0..3 {
            slots_probe.push(None);
        }
        let (slots_ptr_off, _slots_len_off) = vec_word_offsets(&slots_probe);

        let (disc_off, elems_ptr_off, elems_len_off) = {
            let mut boxed_vec: Vec<VmValue> = Vec::with_capacity(7);
            for _ in 0..3 {
                boxed_vec.push(VmValue::null());
            }
            let vec_ptr = boxed_vec.as_ptr() as usize;
            let repr = varn_types::vm_value::ArrayRepr::Boxed(boxed_vec);
            let repr_size = std::mem::size_of::<varn_types::vm_value::ArrayRepr>();
            let base = &repr as *const _ as *const u8;
            let word = std::mem::size_of::<usize>();

            let disc = unsafe { *base };
            assert_eq!(disc, 0, "ArrayRepr::Boxed discriminant must be 0");

            let mut ptr_off = usize::MAX;
            let mut len_off = usize::MAX;
            let mut off = word;
            while off + word <= repr_size {
                let w = unsafe { *(base.add(off) as *const usize) };
                if w == vec_ptr {
                    ptr_off = off;
                } else if w == 3 {
                    len_off = off;
                }
                off += word;
            }
            assert!(
                ptr_off != usize::MAX && len_off != usize::MAX,
                "ArrayRepr::Boxed Vec layout probe failed"
            );

            let probed_ptr = unsafe { *(base.add(ptr_off) as *const usize) };
            let probed_len = unsafe { *(base.add(len_off) as *const usize) };
            assert_eq!(
                probed_ptr, vec_ptr,
                "elems_ptr_off probe read-back mismatch"
            );
            assert_eq!(probed_len, 3, "elems_len_off probe read-back mismatch");

            (0usize, ptr_off, len_off)
        };

        {
            let mut i64_vec: Vec<i64> = Vec::with_capacity(7);
            for _ in 0..3 {
                i64_vec.push(0);
            }
            let i64_ptr = i64_vec.as_ptr() as usize;
            let repr = varn_types::vm_value::ArrayRepr::I64(i64_vec);
            let base = &repr as *const _ as *const u8;
            let disc = unsafe { *base.add(disc_off) };
            assert_eq!(
                disc, 1,
                "ArrayRepr::I64 discriminant must read as 1 at the probed disc_off"
            );
            let probed_ptr = unsafe { *(base.add(elems_ptr_off) as *const usize) };
            let probed_len = unsafe { *(base.add(elems_len_off) as *const usize) };
            assert_eq!(
                probed_ptr, i64_ptr,
                "ArrayRepr::I64 Vec ptr does not land at the Boxed-probed elems_ptr_off"
            );
            assert_eq!(
                probed_len, 3,
                "ArrayRepr::I64 Vec len does not land at the Boxed-probed elems_len_off"
            );
        }
        {
            let mut f64_vec: Vec<f64> = Vec::with_capacity(7);
            for _ in 0..3 {
                f64_vec.push(0.0);
            }
            let f64_ptr = f64_vec.as_ptr() as usize;
            let repr = varn_types::vm_value::ArrayRepr::F64(f64_vec);
            let base = &repr as *const _ as *const u8;
            let disc = unsafe { *base.add(disc_off) };
            assert_eq!(
                disc, 2,
                "ArrayRepr::F64 discriminant must read as 2 at the probed disc_off"
            );
            let probed_ptr = unsafe { *(base.add(elems_ptr_off) as *const usize) };
            let probed_len = unsafe { *(base.add(elems_len_off) as *const usize) };
            assert_eq!(
                probed_ptr, f64_ptr,
                "ArrayRepr::F64 Vec ptr does not land at the Boxed-probed elems_ptr_off"
            );
            assert_eq!(
                probed_len, 3,
                "ArrayRepr::F64 Vec len does not land at the Boxed-probed elems_len_off"
            );
        }

        let arr = varn_types::vm_value::VmArray::new(vec![VmValue::null()]);
        let rcbox = Rc::as_ptr(&arr.0) as usize - 2 * std::mem::size_of::<usize>();
        let slot: Option<HeapObj> = Some(HeapObj::Array(arr));
        let size = std::mem::size_of::<Option<HeapObj>>();
        let bytes = unsafe { std::slice::from_raw_parts(&slot as *const _ as *const u8, size) };
        let array_tag = bytes[0] as usize;
        let payload_off = (0..=size - 8)
            .find(|&off| usize::from_ne_bytes(bytes[off..off + 8].try_into().unwrap()) == rcbox)
            .expect("array payload probe failed");

        let none_slot: Option<HeapObj> = None;
        let none_tag = unsafe { *(&none_slot as *const _ as *const u8) } as usize;
        assert_ne!(array_tag, none_tag, "Option<HeapObj> niche probe failed");

        varn_jit::JitArrayLayout {
            slots_vec_off: 2 * std::mem::size_of::<usize>()
                + std::mem::offset_of!(HeapInner, objects),
            nursery_slots_vec_off: 2 * std::mem::size_of::<usize>()
                + std::mem::offset_of!(HeapInner, nursery)
                + crate::nursery::Nursery::objects_vec_byte_offset(),
            slots_ptr_off,
            slot_size: size,
            array_tag,
            payload_off,
            disc_off,
            elems_ptr_off,
            elems_len_off,
        }
    }

    pub(crate) fn jit_object_layout() -> varn_jit::JitObjectLayout {
        const SENTINEL_FIELD: u64 = 0xFEED_BEEF_CAFE_1234;
        const TAIL: usize = 3;

        let shape = varn_types::Shape::create(None, std::collections::HashMap::new());
        let shape_id = shape.id;
        let oref = ObjRef::with_shape(Rc::clone(&shape), vec![VmValue(SENTINEL_FIELD); TAIL]);

        let rcbox = Rc::as_ptr(&oref.0) as *const u8 as usize - 2 * std::mem::size_of::<usize>();
        let shape_ptr = Rc::as_ptr(&shape) as *const u8 as usize - 2 * std::mem::size_of::<usize>();

        let block = unsafe { std::slice::from_raw_parts(rcbox as *const u8, 80) };
        let word_at =
            |off: usize| -> u64 { u64::from_ne_bytes(block[off..off + 8].try_into().unwrap()) };

        let values_off = (0..=72)
            .step_by(8)
            .find(|&off| word_at(off) == SENTINEL_FIELD)
            .expect("object tail probe failed: no sentinel field found");
        let shape_off = (0..=72)
            .step_by(8)
            .find(|&off| word_at(off) as usize == shape_ptr)
            .expect("object shape probe failed");
        let len_off = (0..=72)
            .step_by(8)
            .find(|&off| (word_at(off) & 0xFFFF_FFFF) as usize == TAIL && off != values_off)
            .expect("object inline_len probe failed");

        let shape_id_off =
            2 * std::mem::size_of::<usize>() + std::mem::offset_of!(varn_types::Shape, id);
        assert_eq!(
            unsafe { *((shape_ptr + shape_id_off) as *const u32) },
            shape_id,
            "shape id offset does not resolve to Shape.id"
        );

        let slot: Option<HeapObj> = Some(HeapObj::Object(oref.clone()));
        let size = std::mem::size_of::<Option<HeapObj>>();
        let bytes = unsafe { std::slice::from_raw_parts(&slot as *const _ as *const u8, size) };
        let object_tag = bytes[0] as usize;
        let payload_off = (0..=size - 8)
            .find(|&off| usize::from_ne_bytes(bytes[off..off + 8].try_into().unwrap()) == rcbox)
            .expect("object payload probe failed");

        let none_tag = unsafe { *(&(None::<HeapObj>) as *const _ as *const u8) } as usize;
        assert_ne!(object_tag, none_tag, "Option<HeapObj> niche probe failed");

        varn_jit::JitObjectLayout {
            object_tag,
            payload_off,
            len_off,
            values_off,
            shape_off,
            shape_id_off,
        }
    }
}

#[cfg(test)]
mod jit_object_layout_tests {
    use super::*;

    #[test]
    fn probed_offsets_reach_the_real_fields() {
        varn_runtime::init_heap();
        let lay = Heap::jit_object_layout();

        let shape = varn_types::Shape::create(None, std::collections::HashMap::new());
        let oref = ObjRef::with_shape(
            Rc::clone(&shape),
            vec![VmValue::from_int(11), VmValue::from_int(22)],
        );
        let slot: Option<HeapObj> = Some(HeapObj::Object(oref.clone()));

        let slot_bytes = unsafe {
            std::slice::from_raw_parts(
                &slot as *const _ as *const u8,
                std::mem::size_of::<Option<HeapObj>>(),
            )
        };
        assert_eq!(slot_bytes[0] as usize, lay.object_tag, "tag byte");

        let rc = usize::from_ne_bytes(
            slot_bytes[lay.payload_off..lay.payload_off + 8]
                .try_into()
                .unwrap(),
        );

        let read_u64 = |addr: usize| unsafe { *(addr as *const u64) };
        let read_u32 = |addr: usize| unsafe { *(addr as *const u32) };

        assert_eq!(read_u32(rc + lay.len_off) as usize, 2, "inline_len");

        let shape_ptr = read_u64(rc + lay.shape_off) as usize;
        assert_eq!(read_u32(shape_ptr + lay.shape_id_off), shape.id, "shape id");

        assert_eq!(read_u64(rc + lay.values_off), VmValue::from_int(11).0);
        assert_eq!(read_u64(rc + lay.values_off + 8), VmValue::from_int(22).0);
    }
}

#[cfg(test)]
mod heap_obj_size_tests {
    use super::super::str::INLINE_STR_CAP;
    use super::*;

    #[test]
    fn heap_obj_slot_stride_is_unchanged() {
        assert_eq!(std::mem::size_of::<HeapObj>(), 48);
        assert!(INLINE_STR_CAP <= u8::MAX as usize, "len field is a u8");
    }
}
