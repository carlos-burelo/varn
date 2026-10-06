use std::ffi::c_void;

#[cfg(target_os = "windows")]
mod imp {
    use super::*;

    pub const MEM_COMMIT: u32 = 0x1000;
    pub const MEM_RESERVE: u32 = 0x2000;
    pub const MEM_RELEASE: u32 = 0x8000;
    pub const PAGE_READWRITE: u32 = 0x04;
    pub const PAGE_EXECUTE_READ: u32 = 0x20;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn VirtualAlloc(
            lpAddress: *const c_void,
            dwSize: usize,
            flAllocationType: u32,
            flProtect: u32,
        ) -> *mut c_void;

        pub fn VirtualProtect(
            lpAddress: *const c_void,
            dwSize: usize,
            flNewProtect: u32,
            lpflOldProtect: *mut u32,
        ) -> i32;

        pub fn VirtualFree(lpAddress: *mut c_void, dwSize: usize, dwFreeType: u32) -> i32;

        pub fn FlushInstructionCache(
            hProcess: *mut c_void,
            lpBaseAddress: *const c_void,
            dwSize: usize,
        ) -> i32;

        pub fn GetCurrentProcess() -> *mut c_void;
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::*;

    pub const PROT_READ: i32 = 1;
    pub const PROT_WRITE: i32 = 2;
    pub const PROT_EXEC: i32 = 4;

    pub const MAP_PRIVATE: i32 = 0x02;
    pub const MAP_FAILED: *mut c_void = !0 as *mut c_void;

    #[cfg(target_os = "macos")]
    pub const MAP_ANON: i32 = 0x1000;
    #[cfg(not(target_os = "macos"))]
    pub const MAP_ANON: i32 = 0x20;

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    pub const MAP_JIT: i32 = 0x0800;
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    pub const MAP_JIT: i32 = 0;

    extern "C" {
        pub fn mmap(
            addr: *mut c_void,
            length: usize,
            prot: i32,
            flags: i32,
            fd: i32,
            offset: isize,
        ) -> *mut c_void;

        pub fn mprotect(addr: *mut c_void, len: usize, prot: i32) -> i32;

        pub fn munmap(addr: *mut c_void, length: usize) -> i32;
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    extern "C" {
        pub fn pthread_jit_write_protect_np(enabled: i32);
    }
}

#[cfg(target_os = "windows")]
pub use imp::*;
#[cfg(not(target_os = "windows"))]
pub use imp::*;
