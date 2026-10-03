use super::*;

/// `may_hold_closure` lo decide el sitio de llamada cuando puede: si el backend
/// sabe que todos los campos son valores desboxados, ninguno es una closure y el
/// barrido que cierra upvalues sobra. El intérprete no lo sabe y pasa `true`.
pub(crate) fn build_with_shape(
    store: &crate::frame_store::FrameStore,
    base: usize,
    start_reg: usize,
    shape: Rc<varn_types::Shape>,
    heap: &mut Heap,
    may_hold_closure: bool,
    is_record: bool,
) -> VmValue {
    let oref = build_shaped(store, base, start_reg, shape, heap, may_hold_closure);
    let obj = if is_record {
        HeapObj::Record(oref)
    } else {
        HeapObj::Object(oref)
    };
    alloc_timed(heap, obj)
}

/// Window-taking sibling of [`build_with_shape`]: the SSA lowering has no
/// contiguous home window for an object literal's values, so it stages a boxed
/// slice. Closure upvalues are still closed against the live frame `store`.
pub(crate) fn build_with_shape_slice(
    store: &crate::frame_store::FrameStore,
    shape: Rc<varn_types::Shape>,
    vals: &[VmValue],
    heap: &mut Heap,
    may_hold_closure: bool,
    is_record: bool,
) -> VmValue {
    if may_hold_closure {
        for &val_nv in vals {
            if val_nv.is_heap() {
                if let Some(HeapObj::VmClosure(nc)) = heap.get(val_nv.as_heap_idx()) {
                    for uv in &nc.upvalues {
                        uv.close(store);
                    }
                }
            }
        }
    }
    let oref = ObjRef::with_shape_slice(shape, vals);
    let obj = if is_record {
        HeapObj::Record(oref)
    } else {
        HeapObj::Object(oref)
    };
    alloc_timed(heap, obj)
}

/// `Heap::alloc` con el tramo anotado: mover el `HeapObj` de 48 bytes y
/// empujarlo al nursery es uno de los candidatos a explicar los ~54 ns que el
/// allocator no explica.
#[inline(always)]
fn alloc_timed(heap: &mut Heap, obj: HeapObj) -> VmValue {
    use crate::alloc_profile as prof;
    if !prof::detail() {
        return VmValue::from_heap_idx(heap.alloc(obj));
    }
    let t0 = prof::read();
    let idx = heap.alloc(obj);
    prof::record(prof::Seg::HeapPush, t0, prof::read());
    VmValue::from_heap_idx(idx)
}

/// Parte común de objeto y record: cerrar las upvalues de los valores que sean
/// closures y copiar los campos dentro del objeto.
///
/// `with_shape_slice`, no `with_shape`: la forma con `Vec` aloca un buffer,
/// lo copia en el almacenamiento inline del objeto y lo tira — una asignación
/// entera de heap por objeto construido, sin ningún fin. Es el mismo derroche
/// que el comentario de `ObjData::with_shape_slice` documenta como corregido
/// para `JSON.parse`, y estaba en el camino principal de creación de objetos.
fn build_shaped(
    store: &crate::frame_store::FrameStore,
    base: usize,
    start_reg: usize,
    shape: Rc<varn_types::Shape>,
    heap: &Heap,
    may_hold_closure: bool,
) -> ObjRef {
    use crate::alloc_profile as prof;
    let count = shape.property_names.len();
    let on = prof::detail();

    let t0 = if on { prof::read() } else { 0 };
    if may_hold_closure {
        for i in 0..count {
            let val_nv = store.box_reg(base, start_reg + i);
            if val_nv.is_heap() {
                // Sin clonar el closure: sólo se leen sus upvalues, y `close`
                // toca el almacén, no el heap.
                if let Some(crate::heap::HeapObj::VmClosure(nc)) = heap.get(val_nv.as_heap_idx()) {
                    for uv in &nc.upvalues {
                        uv.close(store);
                    }
                }
            }
        }
    }
    if on {
        prof::record(prof::Seg::ClosureScan, t0, prof::read());
    }

    let t1 = if on { prof::read() } else { 0 };
    // El frame por clases no es contiguo: se boxea a un Vec para el slice.
    // Solo lo usa el helper JIT (vía `build_with_shape`); el intérprete va
    // por `alloc_*_with_shape_slice` con su propio boxeo.
    let vals: Vec<VmValue> = (0..count)
        .map(|i| store.box_reg(base, start_reg + i))
        .collect();
    let oref = ObjRef::with_shape_slice(shape, &vals);
    if on {
        prof::record(prof::Seg::ObjDataAlloc, t1, prof::read());
    }
    oref
}
