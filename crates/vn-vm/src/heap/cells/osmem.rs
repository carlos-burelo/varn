use std::ptr::NonNull;
use vn_types::cell::BLOCK_BYTES;

pub(super) const MAPPING_BYTES: usize = BLOCK_BYTES * 2;

pub(super) fn map_block() -> (NonNull<u8>, NonNull<u8>) {
    let base = unsafe { reserve(MAPPING_BYTES) };
    let base = NonNull::new(base).unwrap_or_else(|| std::alloc::handle_alloc_error(block_layout()));
    let aligned = (base.as_ptr() as usize + (BLOCK_BYTES - 1)) & !(BLOCK_BYTES - 1);
    let aligned = unsafe { NonNull::new_unchecked(aligned as *mut u8) };
    if !unsafe { commit(aligned.as_ptr(), BLOCK_BYTES) } {
        unsafe { release(base.as_ptr(), MAPPING_BYTES) };
        std::alloc::handle_alloc_error(block_layout());
    }
    (aligned, base)
}

pub(super) fn unmap_block(base: NonNull<u8>) {
    unsafe { release(base.as_ptr(), MAPPING_BYTES) };
}

pub(super) fn block_layout() -> std::alloc::Layout {
    std::alloc::Layout::from_size_align(MAPPING_BYTES, BLOCK_BYTES).expect("block mapping")
}

#[cfg(windows)]
unsafe fn reserve(bytes: usize) -> *mut u8 {
    use std::ffi::c_void;
    const MEM_RESERVE: u32 = 0x2000;
    const PAGE_NOACCESS: u32 = 0x01;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
    }
    unsafe { VirtualAlloc(std::ptr::null_mut(), bytes, MEM_RESERVE, PAGE_NOACCESS) as *mut u8 }
}

#[cfg(windows)]
unsafe fn commit(ptr: *mut u8, bytes: usize) -> bool {
    use std::ffi::c_void;
    const MEM_COMMIT: u32 = 0x1000;
    const PAGE_READWRITE: u32 = 0x04;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
    }
    !unsafe { VirtualAlloc(ptr as *mut c_void, bytes, MEM_COMMIT, PAGE_READWRITE) }.is_null()
}

#[cfg(windows)]
unsafe fn release(ptr: *mut u8, _bytes: usize) {
    use std::ffi::c_void;
    const MEM_RELEASE: u32 = 0x8000;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn VirtualFree(address: *mut c_void, size: usize, kind: u32) -> i32;
    }
    unsafe { VirtualFree(ptr as *mut c_void, 0, MEM_RELEASE) };
}

#[cfg(unix)]
unsafe fn reserve(bytes: usize) -> *mut u8 {
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            bytes,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        std::ptr::null_mut()
    } else {
        ptr as *mut u8
    }
}

#[cfg(unix)]
unsafe fn commit(_ptr: *mut u8, _bytes: usize) -> bool {
    true
}

#[cfg(unix)]
unsafe fn release(ptr: *mut u8, bytes: usize) {
    unsafe { libc::munmap(ptr as *mut libc::c_void, bytes) };
}
