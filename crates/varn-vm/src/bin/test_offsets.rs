use varn_vm::exec::ctx::ExecCtx;

fn main() {
    let dummy = std::mem::MaybeUninit::<ExecCtx>::uninit();
    let dummy_ptr = dummy.as_ptr();
    unsafe {
        println!("Offset of stack: {}", (std::ptr::addr_of!((*dummy_ptr).stack) as usize) - (dummy_ptr as usize));
        println!("Offset of frames: {}", (std::ptr::addr_of!((*dummy_ptr).frames) as usize) - (dummy_ptr as usize));
        println!("Offset of open_upvalues: {}", (std::ptr::addr_of!((*dummy_ptr).open_upvalues) as usize) - (dummy_ptr as usize));
        println!("Offset of pending_constructors: {}", (std::ptr::addr_of!((*dummy_ptr).pending_constructors) as usize) - (dummy_ptr as usize));
    }
}
