use super::*;

pub(crate) fn bundle_rest_args(
    proto: &FunctionProto,
    arg_count: &mut usize,
    staging: &mut Vec<VmValue>,
    heap: &mut Heap,
) {
    let arity = proto.arity;
    if proto.has_rest {
        let rest_idx = arity.saturating_sub(1);
        if *arg_count > rest_idx {
            let num_to_bundle = *arg_count - rest_idx;
            let start = staging.len() - num_to_bundle;
            let items: Vec<VmValue> = staging.drain(start..).collect();
            let nv = heap.alloc_array_repr(false, varn_types::vm_value::ArrayRepr::boxed(items));
            staging.push(nv);
            *arg_count = rest_idx + 1;
        } else {
            for _ in *arg_count..rest_idx {
                staging.push(VmValue::null());
            }
            let nv =
                heap.alloc_array_repr(false, varn_types::vm_value::ArrayRepr::boxed(Vec::new()));
            staging.push(nv);
            *arg_count = rest_idx + 1;
        }
    } else if *arg_count < arity {
        for _ in *arg_count..arity {
            staging.push(VmValue::null());
        }
        *arg_count = arity;
    }
}

pub(crate) fn expand_spread_args(
    heap: &Heap,
    args: impl IntoIterator<Item = VmValue>,
    out: &mut Vec<VmValue>,
) {
    for nv in args {
        let inner = match heap.heap_ref(nv).and_then(|i| heap.get(i)) {
            Some(HeapObj::Spread(inner)) => *inner,
            _ => nv,
        };
        match heap.heap_ref(inner).and_then(|i| heap.get(i)) {
            Some(HeapObj::Array(arr) | HeapObj::Tuple(arr)) => {
                out.extend((0..arr.len()).filter_map(|i| arr.get_vm(i)));
            }
            _ => out.push(inner),
        }
    }
}
