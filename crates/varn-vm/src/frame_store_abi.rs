










use crate::frame_store::FrameStore;
use crate::value::VmValue;

const _: () = {
    assert!(
        core::mem::size_of::<VmValue>() == core::mem::size_of::<varn_abi::AbiValue>(),
        "VmValue/AbiValue: mismo layout 16 B"
    );
    assert!(
        core::mem::align_of::<VmValue>() == core::mem::align_of::<varn_abi::AbiValue>(),
        "VmValue/AbiValue: mismo align"
    );
};


pub fn abi_stacks(store: &FrameStore) -> varn_abi::AbiStacks {
    let gpr = store.gpr.as_ptr() as *mut i64;
    let fpr = store.fpr.as_ptr() as *mut f64;
    let refs = store.refs.as_ptr() as *mut u32;
    let dyn_ = store.dyn_.as_ptr() as *mut varn_abi::AbiValue;
    
    
    unsafe {
        varn_abi::AbiStacks {
            gpr,
            gpr_end: gpr.add(store.gpr.len()),
            fpr,
            fpr_end: fpr.add(store.fpr.len()),
            refs,
            refs_end: refs.add(store.refs.len()),
            dyn_,
            dyn_end: dyn_.add(store.dyn_.len()),
        }
    }
}



pub fn debug_check_stacks(store: &FrameStore) -> bool {
    let v = abi_stacks(store);
    
    
    let ok = |base: *mut u8, end: *mut u8| (end as usize) >= (base as usize);
    ok(v.gpr as *mut u8, v.gpr_end as *mut u8)
        && ok(v.fpr as *mut u8, v.fpr_end as *mut u8)
        && ok(v.refs as *mut u8, v.refs_end as *mut u8)
        && ok(v.dyn_ as *mut u8, v.dyn_end as *mut u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use varn_types::register_meta::SlotKind;

    #[test]
    fn rangos_cubren_tramos_vivos() {
        let proto = std::rc::Rc::new(varn_types::FunctionProto {
            register_count: 2,
            register_meta: vec![
                varn_types::register_meta::RegisterMeta {
                    kind: SlotKind::Int,
                },
                varn_types::register_meta::RegisterMeta {
                    kind: SlotKind::Dynamic,
                },
            ],
            ..Default::default()
        });
        let mut s = FrameStore::new();
        s.push_frame(&proto);
        let v = abi_stacks(&s);
        unsafe {
            assert!(v.gpr_end.offset_from(v.gpr) >= 1);
            assert!(v.dyn_end.offset_from(v.dyn_) >= 1);
        }
    }
}
