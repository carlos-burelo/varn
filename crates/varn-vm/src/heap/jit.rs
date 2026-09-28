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

        // Derivado, no escaneado: `ArrayRepr` es `repr(C, u8)` propio
        // (`varn_types::vm_value::ArrayRepr::{DISC_OFF, ELEMS_PTR_OFF,
        // ELEMS_LEN_OFF}`). El tripwire lee un valor real: si la
        // representación cambia, falla aquí en voz alta, no en código emitido.
        let (disc_off, elems_ptr_off, elems_len_off) = {
            use varn_types::vm_value::ArrayRepr;
            let mut boxed_vec: Vec<VmValue> = Vec::with_capacity(7);
            for _ in 0..3 {
                boxed_vec.push(VmValue::null());
            }
            let vec_ptr = boxed_vec.as_ptr() as usize;
            let repr = ArrayRepr::Boxed(boxed_vec);
            let base = &repr as *const _ as *const u8;
            let disc = unsafe { *base.add(ArrayRepr::DISC_OFF) };
            assert_eq!(disc, 0, "ArrayRepr::Boxed discriminant must read 0 at DISC_OFF");
            let probed_ptr =
                unsafe { *(base.add(ArrayRepr::ELEMS_PTR_OFF) as *const usize) };
            let probed_len =
                unsafe { *(base.add(ArrayRepr::ELEMS_LEN_OFF) as *const usize) };
            assert_eq!(
                probed_ptr, vec_ptr,
                "ELEMS_PTR_OFF tripwire: Boxed Vec ptr mismatch"
            );
            assert_eq!(probed_len, 3, "ELEMS_LEN_OFF tripwire: Boxed Vec len mismatch");

            // La carga útil es una unión: I64/F64 comparten palabras con Boxed.
            // Solo se verifica su tag en el mismo offset.
            let i64_repr = ArrayRepr::I64(vec![0, 0, 0]);
            let i64_disc =
                unsafe { *(&i64_repr as *const _ as *const u8).add(ArrayRepr::DISC_OFF) };
            assert_eq!(i64_disc, 1, "ArrayRepr::I64 discriminant must read 1");
            let f64_repr = ArrayRepr::F64(vec![0.0, 0.0, 0.0]);
            let f64_disc =
                unsafe { *(&f64_repr as *const _ as *const u8).add(ArrayRepr::DISC_OFF) };
            assert_eq!(f64_disc, 2, "ArrayRepr::F64 discriminant must read 2");

            (
                ArrayRepr::DISC_OFF,
                ArrayRepr::ELEMS_PTR_OFF,
                ArrayRepr::ELEMS_LEN_OFF,
            )
        };

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

        let shape = varn_types::Shape::create(None, rustc_hash::FxHashMap::default());
        let shape_id = shape.id;
        // Both words carry the sentinel, so the tripwire below lands on the
        // tail whichever half it reads first.
        let oref = ObjRef::with_shape(
            Rc::clone(&shape),
            vec![VmValue::from_raw_parts(SENTINEL_FIELD, SENTINEL_FIELD); TAIL],
        );

        let rcbox = Rc::as_ptr(&oref.0) as *const u8 as usize - 2 * std::mem::size_of::<usize>();
        let shape_ptr = Rc::as_ptr(&shape) as *const u8 as usize - 2 * std::mem::size_of::<usize>();

        // Derivado, no escaneado: `ObjData`/`Shape` son `repr(C)` propios
        // (`OBJ_VALUES_OFF`, `OBJ_SHAPE_OFF`, `OBJ_INLINE_LEN_OFF`,
        // `SHAPE_ID_OFF`). Tripwires sobre un valor real.
        let values_off = varn_types::OBJ_VALUES_OFF;
        let shape_off = varn_types::OBJ_SHAPE_OFF;
        let len_off = varn_types::OBJ_INLINE_LEN_OFF;
        let block = unsafe { std::slice::from_raw_parts(rcbox as *const u8, 80) };
        let word_at =
            |off: usize| -> u64 { u64::from_ne_bytes(block[off..off + 8].try_into().unwrap()) };
        assert_eq!(
            word_at(values_off),
            SENTINEL_FIELD,
            "OBJ_VALUES_OFF tripwire: tail sentinel not at derived offset"
        );
        assert!(
            (word_at(len_off) & 0xFFFF_FFFF) as usize == TAIL && len_off != values_off,
            "OBJ_INLINE_LEN_OFF tripwire: inline_len not at derived offset"
        );

        let shape_id_off =
            2 * std::mem::size_of::<usize>() + varn_types::SHAPE_ID_OFF;
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

        // Probe HeapObj::Instance layout facts
        let dummy_cls = varn_types::ClassObj::new_rc("__probe_class");
        let inst_ref = varn_types::value::InstanceRef::alloc(dummy_cls);
        let inst_rcbox =
            Rc::as_ptr(&inst_ref.0) as *const u8 as usize - 2 * std::mem::size_of::<usize>();
        let inst_slot: Option<HeapObj> = Some(HeapObj::Instance(inst_ref.clone()));
        let inst_bytes =
            unsafe { std::slice::from_raw_parts(&inst_slot as *const _ as *const u8, size) };
        let instance_tag = inst_bytes[0] as usize;
        let instance_payload_off = (0..=size - 8)
            .find(|&off| {
                usize::from_ne_bytes(inst_bytes[off..off + 8].try_into().unwrap()) == inst_rcbox
            })
            .expect("instance payload probe failed");

        let raw_payload_ptr = inst_ref.raw_payload_ptr() as usize;
        let instance_values_off = raw_payload_ptr - inst_rcbox;
        // `InstanceData { class_id: u32, payload_size: u32, payload }` — an
        // 8-byte header immediately before the payload.
        let instance_class_id_off = instance_values_off - 8;
        assert_eq!(
            unsafe { *((inst_rcbox + instance_class_id_off) as *const u32) },
            inst_ref.class_id,
            "instance class_id offset does not resolve to InstanceData.class_id"
        );

        varn_jit::JitObjectLayout {
            object_tag,
            instance_tag,
            payload_off,
            instance_payload_off,
            len_off,
            values_off,
            instance_values_off,
            instance_class_id_off,
            shape_off,
            shape_id_off,
        }
    }
}
