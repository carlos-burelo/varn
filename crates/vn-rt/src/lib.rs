use std::io::Write;

unsafe extern "C" {

    fn _varn_main() -> i64;
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let code = unsafe { _varn_main() };
    code as i32
}

fn print_line(text: &[u8]) {
    let mut out = std::io::stdout();
    let _ = out.write_all(text);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn varn_rt_print(ptr: *const u8, len: usize) {
    if ptr.is_null() || len == 0 {
        print_line(b"");
    } else {
        print_line(unsafe { std::slice::from_raw_parts(ptr, len) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn varn_rt_print_int(value: i64) {
    print_line(value.to_string().as_bytes());
}

#[unsafe(no_mangle)]
pub extern "C" fn varn_rt_print_bool(value: i64) {
    print_line(if value != 0 { b"true" } else { b"false" });
}

#[unsafe(no_mangle)]
pub extern "C" fn varn_rt_print_null() {
    print_line(b"null");
}

#[repr(C)]
pub struct StrResult {
    pub ptr: *const u8,
    pub len: usize,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn varn_rt_str_concat(
    a_ptr: *const u8,
    a_len: usize,
    b_ptr: *const u8,
    b_len: usize,
    out: *mut StrResult,
) {
    let mut vec = Vec::with_capacity(a_len + b_len);
    if !a_ptr.is_null() && a_len > 0 {
        vec.extend_from_slice(unsafe { std::slice::from_raw_parts(a_ptr, a_len) });
    }
    if !b_ptr.is_null() && b_len > 0 {
        vec.extend_from_slice(unsafe { std::slice::from_raw_parts(b_ptr, b_len) });
    }
    let len = vec.len();
    let ptr = Box::into_raw(vec.into_boxed_slice()) as *const u8;
    unsafe { out.write(StrResult { ptr, len }) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn varn_rt_panic(ptr: *const u8, len: usize) -> ! {
    if !ptr.is_null() && len > 0 {
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        let msg = String::from_utf8_lossy(slice);
        eprintln!("Varn native runtime panic: {msg}");
    } else {
        eprintln!("Varn native runtime panic");
    }
    std::process::exit(1);
}
