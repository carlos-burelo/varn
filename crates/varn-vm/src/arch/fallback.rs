//! Generic portable fallback for setjmp/longjmp using libc / C runtime.

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JmpBuf {
    storage: [usize; 64],
}

impl Default for JmpBuf {
    fn default() -> Self {
        Self { storage: [0; 64] }
    }
}

extern "C" {
    fn setjmp(env: *mut JmpBuf) -> std::ffi::c_int;
    fn longjmp(env: *const JmpBuf, val: std::ffi::c_int) -> !;
}

#[inline(always)]
pub unsafe fn vm_setjmp(buf: *mut JmpBuf) -> i32 {
    setjmp(buf) as i32
}

#[inline(always)]
pub unsafe fn vm_longjmp(buf: *const JmpBuf, val: i32) -> ! {
    longjmp(buf, val as std::ffi::c_int)
}
