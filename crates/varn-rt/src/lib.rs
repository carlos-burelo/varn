use std::io::Write;

extern "C" {

    fn _varn_main() -> i64;
}

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let code = unsafe { _varn_main() };
    code as i32
}

#[no_mangle]
pub unsafe extern "C" fn varn_rt_print(ptr: *const u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        let slice = std::slice::from_raw_parts(ptr, len);
        let _ = std::io::stdout().write_all(slice);
    }
    let _ = std::io::stdout().write_all(b"\n");
    let _ = std::io::stdout().flush();
}

#[no_mangle]
pub extern "C" fn varn_rt_print_int(val: i64) {
    println!("{val}");
}

#[no_mangle]
pub extern "C" fn varn_rt_print_bool(val: i64) {
    if val != 0 {
        println!("true");
    } else {
        println!("false");
    }
}

#[repr(C)]
pub struct StrResult {
    pub ptr: *const u8,
    pub len: usize,
}

#[no_mangle]
pub unsafe extern "C" fn varn_rt_str_concat(
    a_ptr: *const u8,
    a_len: usize,
    b_ptr: *const u8,
    b_len: usize,
) -> StrResult {
    let mut vec = Vec::with_capacity(a_len + b_len);
    if !a_ptr.is_null() && a_len > 0 {
        vec.extend_from_slice(std::slice::from_raw_parts(a_ptr, a_len));
    }
    if !b_ptr.is_null() && b_len > 0 {
        vec.extend_from_slice(std::slice::from_raw_parts(b_ptr, b_len));
    }
    let len = vec.len();
    let ptr = Box::into_raw(vec.into_boxed_slice()) as *const u8;
    StrResult { ptr, len }
}

#[no_mangle]
pub unsafe extern "C" fn varn_rt_panic(ptr: *const u8, len: usize) -> ! {
    if !ptr.is_null() && len > 0 {
        let slice = std::slice::from_raw_parts(ptr, len);
        let msg = String::from_utf8_lossy(slice);
        eprintln!("Varn native runtime panic: {msg}");
    } else {
        eprintln!("Varn native runtime panic");
    }
    std::process::exit(1);
}
