use super::*;




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
