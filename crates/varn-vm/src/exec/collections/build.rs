use super::*;

pub(crate) fn build_with_shape(
    store: &crate::frame_store::FrameStore,
    base: usize,
    start_reg: usize,
    shape: Rc<varn_types::Shape>,
    heap: &mut Heap,
    is_record: bool,
) -> VmValue {
    let vals: Vec<VmValue> = (0..shape.property_names.len())
        .map(|i| store.box_reg(base, start_reg + i))
        .collect();
    build_with_shape_slice(shape, &vals, heap, is_record)
}

pub(crate) fn build_with_shape_slice(
    shape: Rc<varn_types::Shape>,
    vals: &[VmValue],
    heap: &mut Heap,
    is_record: bool,
) -> VmValue {
    heap.alloc_object_cell(is_record, shape, vals.len(), vals)
}
