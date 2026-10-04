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
    let vals: Vec<VmValue> = (0..shape.property_names.len())
        .map(|i| store.box_reg(base, start_reg + i))
        .collect();
    build_with_shape_slice(store, shape, &vals, heap, may_hold_closure, is_record)
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
        close_captured_upvalues(store, vals, heap);
    }
    heap.alloc_object_cell(is_record, shape, vals.len(), vals)
}

/// Sin clonar el closure: sólo se leen sus upvalues, y `close` toca el
/// almacén, no el heap.
fn close_captured_upvalues(store: &crate::frame_store::FrameStore, vals: &[VmValue], heap: &Heap) {
    for &val_nv in vals {
        if val_nv.is_heap() {
            if let Some(HeapObj::VmClosure(nc)) = heap.get(val_nv.as_heap()) {
                for uv in &nc.upvalues {
                    uv.close(store);
                }
            }
        }
    }
}
