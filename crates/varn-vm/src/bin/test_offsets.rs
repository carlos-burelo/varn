use varn_vm::globals::GlobalStore;

fn main() {
    let mut globals = GlobalStore::new();
    // Push some values to get distinct len and cap
    globals.set_by_index(0, varn_types::VmValue::null());
    globals.set_by_index(1, varn_types::VmValue::null());
    globals.set_by_index(2, varn_types::VmValue::null());
    
    let len = globals.values.len();
    let cap = globals.values.capacity();
    println!("Logical: len = {}, cap = {}", len, cap);
    
    let vec_ref = &globals.values;
    let ptr = vec_ref as *const _ as *const usize;
    unsafe {
        println!("Word 0 (offset 0): {:#x}", *ptr);
        println!("Word 1 (offset 8): {}", *ptr.add(1));
        println!("Word 2 (offset 16): {}", *ptr.add(2));
    }
}
