use super::obj::HeapObj;
use super::structs::{Heap, HeapInner};
use crate::value::VmValue;
use std::rc::Rc;

/// Prefijo control del `RcBox` (`strong` + `weak`): el `Rc` guarda el puntero
/// control y `Rc::as_ptr` deriva el valor como control+16. Supuesto
/// codebase-wide, validado contra el heap vivo en `ExecCtx::new`.
const RCBOX_PREFIX: usize = 2 * std::mem::size_of::<usize>();

impl Heap {
    pub(crate) fn young_len_byte_offset_from_rcbox() -> usize {
        RCBOX_PREFIX
            + std::mem::offset_of!(HeapInner, young)
            + super::young::YoungGen::born_len_byte_offset()
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
        // Único hecho medido sobre `Vec` (memoria ajena): el orden de sus
        // tres palabras. Vale para TODO `Vec<T>` del binario, incluido el de
        // `ArrayRepr`: misma definición, mismo orden.
        let (slots_ptr_off, vec_len_off) = vec_word_offsets(&slots_probe);

        // Derivado, no escaneado: la carga útil de `ArrayRepr` es una unión
        // de tres `Vec` que abre en el primer offset alineado a 8 tras el tag
        // (`ELEMS_UNION_OFF`), y dentro de ella rigen las palabras medidas
        // arriba. Tripwire sobre un valor real.
        let (disc_off, elems_ptr_off, elems_len_off) = {
            use varn_types::vm_value::ArrayRepr;
            let mut boxed_vec: Vec<VmValue> = Vec::with_capacity(7);
            for _ in 0..3 {
                boxed_vec.push(VmValue::null());
            }
            let vec_ptr = boxed_vec.as_ptr() as usize;
            let repr = ArrayRepr::Boxed(varn_types::BoxedElems::new(boxed_vec));
            let base = &repr as *const _ as *const u8;
            let disc = unsafe { *base.add(ArrayRepr::DISC_OFF) };
            assert_eq!(
                disc, 0,
                "ArrayRepr::Boxed discriminant must read 0 at DISC_OFF"
            );
            let ptr_off = ArrayRepr::ELEMS_UNION_OFF + slots_ptr_off;
            let len_off = ArrayRepr::ELEMS_UNION_OFF + vec_len_off;
            let probed_ptr = unsafe { *(base.add(ptr_off) as *const usize) };
            let probed_len = unsafe { *(base.add(len_off) as *const usize) };
            assert_eq!(
                probed_ptr, vec_ptr,
                "elems tripwire: Boxed Vec ptr mismatch"
            );
            assert_eq!(probed_len, 3, "elems tripwire: Boxed Vec len mismatch");

            // La unión se comparte: I64/F64 solo verifican su tag.
            let i64_repr = ArrayRepr::I64(vec![0, 0, 0]);
            let i64_disc =
                unsafe { *(&i64_repr as *const _ as *const u8).add(ArrayRepr::DISC_OFF) };
            assert_eq!(i64_disc, 1, "ArrayRepr::I64 discriminant must read 1");
            let f64_repr = ArrayRepr::F64(vec![0.0, 0.0, 0.0]);
            let f64_disc =
                unsafe { *(&f64_repr as *const _ as *const u8).add(ArrayRepr::DISC_OFF) };
            assert_eq!(f64_disc, 2, "ArrayRepr::F64 discriminant must read 2");

            (ArrayRepr::DISC_OFF, ptr_off, len_off)
        };

        let mut repr_cell = std::mem::MaybeUninit::<varn_types::vm_value::ArrayRepr>::uninit();
        let arr = unsafe {
            varn_types::VmArray::init_at(
                repr_cell.as_mut_ptr(),
                varn_types::vm_value::ArrayRepr::boxed(vec![VmValue::null()]),
            )
        };
        let data = repr_cell.as_ptr() as usize;
        let slot: Option<HeapObj> = Some(HeapObj::Array(arr));
        let size = std::mem::size_of::<Option<HeapObj>>();
        let bytes = unsafe { std::slice::from_raw_parts(&slot as *const _ as *const u8, size) };
        let array_tag = bytes[0] as usize;
        let payload_off = (0..=size - 8)
            .find(|&off| usize::from_ne_bytes(bytes[off..off + 8].try_into().unwrap()) == data)
            .expect("array payload probe failed")
            + super::cells::HEADER_BYTES;
        unsafe { arr.drop_at() };

        let none_slot: Option<HeapObj> = None;
        let none_tag = unsafe { *(&none_slot as *const _ as *const u8) } as usize;
        assert_ne!(array_tag, none_tag, "Option<HeapObj> niche probe failed");

        let str_slot: Option<HeapObj> = Some(HeapObj::Str(super::str::HeapStr::inline("")));
        let str_tag = unsafe { *(&str_slot as *const _ as *const u8) } as usize;
        assert_ne!(str_tag, none_tag, "Option<HeapObj> niche probe failed");
        assert_ne!(
            str_tag, array_tag,
            "HeapObj::Str and HeapObj::Array share a tag"
        );

        varn_jit::JitArrayLayout {
            state_off: super::cells::STATE_OFF,
            kind_off: super::cells::KIND_OFF,
            young_state: super::cells::SlotState::Young as usize,
            vec_ptr_off: slots_ptr_off,
            array_tag,
            str_tag,
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
        // tail whichever half it reads first. The object is laid out in a
        // local buffer the way a heap cell lays one out after its HeapObj.
        let mut probe = vec![0u64; varn_types::ObjData::bytes_for(TAIL) / 8];
        let oref = unsafe {
            varn_types::ObjData::init_at(
                probe.as_mut_ptr() as *mut u8,
                Rc::clone(&shape),
                TAIL,
                &[VmValue::from_raw_parts(SENTINEL_FIELD, SENTINEL_FIELD); TAIL],
            )
        };
        let data = probe.as_ptr() as usize;
        let shape_ptr = Rc::as_ptr(&shape) as *const u8 as usize - RCBOX_PREFIX;

        // Derivado, no escaneado: offsets propios (`OBJ_*`) desde el ObjData;
        // la forma sigue siendo un `Rc`, así que su id se alcanza desde el
        // puntero control. Tripwires sobre un valor real.
        let values_off = varn_types::OBJ_VALUES_OFF;
        let shape_off = varn_types::OBJ_SHAPE_OFF;
        let len_off = varn_types::OBJ_INLINE_LEN_OFF;
        let block = unsafe { std::slice::from_raw_parts(data as *const u8, 64) };
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
        assert_eq!(
            word_at(shape_off) as usize,
            shape_ptr,
            "OBJ_SHAPE_OFF tripwire: shape control not at derived offset"
        );

        let shape_id_off = RCBOX_PREFIX + varn_types::SHAPE_ID_OFF;
        assert_eq!(
            unsafe { *((shape_ptr + shape_id_off) as *const u32) },
            shape_id,
            "shape id offset does not resolve to Shape.id"
        );

        let slot: Option<HeapObj> = Some(HeapObj::Object(oref));
        let size = std::mem::size_of::<Option<HeapObj>>();
        let bytes = unsafe { std::slice::from_raw_parts(&slot as *const _ as *const u8, size) };
        let object_tag = bytes[0] as usize;
        let payload_off = (0..=size - 8)
            .find(|&off| usize::from_ne_bytes(bytes[off..off + 8].try_into().unwrap()) == data)
            .expect("object payload probe failed")
            + super::cells::HEADER_BYTES;
        unsafe { varn_types::ObjData::drop_at(oref) };

        let none_tag = unsafe { *(&(None::<HeapObj>) as *const _ as *const u8) } as usize;
        assert_ne!(object_tag, none_tag, "Option<HeapObj> niche probe failed");

        // Probe HeapObj::Instance layout facts on an instance laid out in a
        // local buffer, the way a heap cell lays one out after its HeapObj.
        const PROBE_CLASS_ID: u32 = 0x005E_ED1D;
        let mut probe = [0u64; 2];
        let inst = unsafe {
            varn_types::value::InstanceData::init_at(
                probe.as_mut_ptr() as *mut u8,
                PROBE_CLASS_ID,
                0,
            )
        };
        let data = probe.as_ptr() as usize;
        let inst_slot: Option<HeapObj> = Some(HeapObj::Instance(inst));
        let inst_bytes =
            unsafe { std::slice::from_raw_parts(&inst_slot as *const _ as *const u8, size) };
        let instance_tag = inst_bytes[0] as usize;
        let instance_payload_off = (0..=size - 8)
            .find(|&off| usize::from_ne_bytes(inst_bytes[off..off + 8].try_into().unwrap()) == data)
            .expect("instance payload probe failed")
            + super::cells::HEADER_BYTES;

        // Derivado: offsets propios (`INST_*`) desde el InstanceData. Tripwire:
        // class_id leído ahí debe coincidir.
        let instance_values_off = varn_types::INST_PAYLOAD_OFF;
        let instance_class_id_off = varn_types::INST_CLASS_ID_OFF;
        assert_eq!(
            unsafe { *((data + instance_class_id_off) as *const u32) },
            PROBE_CLASS_ID,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Los hechos derivados coinciden con lo medido: si un cambio de
    /// representación mueve un offset propio, falla aquí (rápido) además del
    /// tripwire en arranque JIT.
    #[test]
    fn layouts_match_derived_consts() {
        use varn_types::vm_value::ArrayRepr;
        let a = Heap::jit_array_layout();
        assert_eq!(a.disc_off, ArrayRepr::DISC_OFF);
        // Unión propia + palabras `Vec` medidas una vez: el frame coincide.
        assert!(a.elems_ptr_off >= ArrayRepr::ELEMS_UNION_OFF);
        assert!(a.elems_len_off >= ArrayRepr::ELEMS_UNION_OFF);
        assert_ne!(a.elems_ptr_off, a.elems_len_off);
        let o = Heap::jit_object_layout();
        assert_eq!(o.values_off, varn_types::OBJ_VALUES_OFF);
        assert_eq!(o.shape_off, varn_types::OBJ_SHAPE_OFF);
        assert_eq!(o.len_off, varn_types::OBJ_INLINE_LEN_OFF);
        assert_eq!(o.shape_id_off, RCBOX_PREFIX + varn_types::SHAPE_ID_OFF);
        assert_eq!(o.instance_values_off, varn_types::INST_PAYLOAD_OFF);
        assert_eq!(o.instance_class_id_off, varn_types::INST_CLASS_ID_OFF);
    }
}
