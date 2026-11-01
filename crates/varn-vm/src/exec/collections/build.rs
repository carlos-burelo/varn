use super::*;

pub(crate) fn build_with_shape(
    store: &crate::frame_store::FrameStore,
    base: usize,
    start_reg: usize,
    shape: Rc<varn_types::Shape>,
    heap: &mut Heap,
) -> VmValue {
    let vals: Vec<VmValue> = (0..shape.property_names.len())
        .map(|i| store.box_reg(base, start_reg + i))
        .collect();
    build_with_shape_slice(shape, &vals, heap)
}

pub(crate) fn build_with_shape_slice(
    shape: Rc<varn_types::Shape>,
    vals: &[VmValue],
    heap: &mut Heap,
) -> VmValue {
    heap.alloc_object_cell(shape, vals.len(), vals)
}
