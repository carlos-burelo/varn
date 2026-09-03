//! Minimal static runtime for Varn AOT compiled binaries.
//!
//! Provides:
//! - Entry point `main` which initializes standard handles and invokes `_varn_main`.
//! - Native runtime helpers: `varn_rt_print`, `varn_rt_print_int`, `varn_rt_print_bool`,
//!   `varn_rt_str_concat`, and `varn_rt_panic`.

use std::io::Write;

extern "C" {
    /// Generated entry point emitted by the AOT compiler into the object file.
    fn _varn_main() -> i64;
}

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let code = unsafe { _varn_main() };
    code as i32
}

/// Print a string slice directly to stdout, followed by a newline.
///
/// # Safety
///
/// `ptr` must point to a valid, readable block of memory of at least `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn varn_rt_print(ptr: *const u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        let slice = std::slice::from_raw_parts(ptr, len);
        let _ = std::io::stdout().write_all(slice);
    }
    let _ = std::io::stdout().write_all(b"\n");
    let _ = std::io::stdout().flush();
}

/// Print an integer directly to stdout, followed by a newline.
#[no_mangle]
pub extern "C" fn varn_rt_print_int(val: i64) {
    println!("{val}");
}

/// Print a boolean directly to stdout, followed by a newline.
#[no_mangle]
pub extern "C" fn varn_rt_print_bool(val: i64) {
    if val != 0 {
        println!("true");
    } else {
        println!("false");
    }
}

/// Concatenate two strings and return a pointer and length packed in a struct / pair.
/// Note: caller expects (ptr: i64, len: i64).
#[repr(C)]
pub struct StrResult {
    pub ptr: *const u8,
    pub len: usize,
}

/// Concatenates two string buffers.
///
/// # Safety
///
/// `a_ptr` and `b_ptr` must point to valid readable blocks of memory of at least
/// `a_len` and `b_len` bytes, respectively.
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

/// Panic with an error message and exit non-zero.
///
/// # Safety
///
/// If `ptr` is non-null, it must point to readable memory of at least `len` bytes.
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
