use crate::frame_store::FrameStore;
use crate::value::VmValue;

const _: () = {
    assert!(
        core::mem::size_of::<VmValue>() == core::mem::size_of::<vn_core::abi::AbiValue>(),
        "VmValue/AbiValue: mismo layout 16 B"
    );
    assert!(
        core::mem::align_of::<VmValue>() == core::mem::align_of::<vn_core::abi::AbiValue>(),
        "VmValue/AbiValue: mismo align"
    );
};

pub fn abi_stacks(store: &FrameStore) -> vn_core::abi::AbiStacks {
    let gpr = store.gpr.as_ptr() as *mut i64;
    let fpr = store.fpr.as_ptr() as *mut f64;
    let refs = store.refs.as_ptr() as *mut u32;
    let dyn_ = store.dyn_.as_ptr() as *mut vn_core::abi::AbiValue;

    unsafe {
        vn_core::abi::AbiStacks {
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
