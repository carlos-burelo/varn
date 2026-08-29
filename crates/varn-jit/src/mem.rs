use std::ffi::c_void;
use std::ptr;

#[cfg(target_os = "windows")]
mod sys {
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
mod sys {
    use super::*;

    pub const PROT_READ: i32 = 1;
    pub const PROT_WRITE: i32 = 2;
    pub const PROT_EXEC: i32 = 4;

    pub const MAP_PRIVATE: i32 = 0x02;
    pub const MAP_FAILED: *mut c_void = !0 as *mut c_void;

    // MAP_ANONYMOUS differs between Linux (0x20) and macOS (MAP_ANON = 0x1000).
    #[cfg(target_os = "macos")]
    pub const MAP_ANON: i32 = 0x1000;
    #[cfg(not(target_os = "macos"))]
    pub const MAP_ANON: i32 = 0x20;

    // Apple Silicon requires MAP_JIT for any mapping that will be made
    // executable. Without it, mprotect(PROT_EXEC) returns ENOTSUP.
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

    // Apple Silicon W^X: before writing JIT code, disable execute protection;
    // after writing, re-enable it. No-op on other platforms.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    extern "C" {
        pub fn pthread_jit_write_protect_np(enabled: i32);
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    #[inline(always)]
    pub unsafe fn pthread_jit_write_protect_np(_enabled: i32) {}
}

pub struct JitBuffer {
    ptr: *mut u8,
    size: usize,
    executable: bool,
}

impl JitBuffer {
    pub fn new(size: usize) -> Result<Self, String> {
        if size == 0 {
            return Err("Size must be greater than zero".to_owned());
        }

        let page_size = 4096;
        let size = (size + page_size - 1) & !(page_size - 1);

        #[cfg(target_os = "windows")]
        {
            let ptr = unsafe {
                sys::VirtualAlloc(
                    ptr::null(),
                    size,
                    sys::MEM_COMMIT | sys::MEM_RESERVE,
                    sys::PAGE_READWRITE,
                )
            };
            if ptr.is_null() {
                return Err("Failed to allocate virtual memory via VirtualAlloc".to_owned());
            }
            Ok(Self {
                ptr: ptr as *mut u8,
                size,
                executable: false,
            })
        }

        #[cfg(not(target_os = "windows"))]
        {
            // On Apple Silicon, MAP_JIT is required for executable mappings.
            // On other Unix systems, MAP_JIT == 0 (no-op OR).
            let flags = sys::MAP_PRIVATE | sys::MAP_ANON | sys::MAP_JIT;
            let ptr = unsafe {
                sys::mmap(
                    ptr::null_mut(),
                    size,
                    sys::PROT_READ | sys::PROT_WRITE,
                    flags,
                    -1,
                    0,
                )
            };
            if ptr == sys::MAP_FAILED {
                return Err("Failed to allocate virtual memory via mmap".to_owned());
            }
            Ok(Self {
                ptr: ptr as *mut u8,
                size,
                executable: false,
            })
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        assert!(!self.executable, "Cannot modify an executable JIT buffer");
        // On Apple Silicon, MAP_JIT memory starts write-protected; disable
        // the guard for the duration of the write, then re-enable in
        // make_executable() via pthread_jit_write_protect_np(1).
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        unsafe {
            sys::pthread_jit_write_protect_np(0);
        }
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.size) }
    }

    pub fn make_executable(&mut self) -> Result<(), String> {
        if self.executable {
            return Ok(());
        }

        #[cfg(target_os = "windows")]
        {
            let mut old_protect = 0;
            let success = unsafe {
                sys::VirtualProtect(
                    self.ptr as *const c_void,
                    self.size,
                    sys::PAGE_EXECUTE_READ,
                    &mut old_protect,
                )
            };
            if success == 0 {
                return Err(
                    "Failed to change memory protection to executable (PAGE_EXECUTE_READ)"
                        .to_owned(),
                );
            }

            unsafe {
                sys::FlushInstructionCache(
                    sys::GetCurrentProcess(),
                    self.ptr as *const c_void,
                    self.size,
                );
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            // On Apple Silicon: re-enable write-execute protection (was disabled
            // in as_mut_slice), then invalidate the instruction cache.
            // On other platforms: mprotect to PROT_READ|PROT_EXEC.
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            {
                unsafe {
                    sys::pthread_jit_write_protect_np(1);
                    let end = self.ptr as usize + self.size;
                    extern "C" {
                        fn sys_icache_invalidate(start: *mut c_void, size: usize);
                    }
                    sys_icache_invalidate(self.ptr as *mut c_void, self.size);
                    let _ = end;
                }
            }

            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
            {
                let res = unsafe {
                    sys::mprotect(
                        self.ptr as *mut c_void,
                        self.size,
                        sys::PROT_READ | sys::PROT_EXEC,
                    )
                };
                if res != 0 {
                    return Err(
                        "Failed to change memory protection to executable (mprotect)".to_owned(),
                    );
                }

                #[cfg(target_arch = "x86_64")]
                unsafe {
                    std::arch::asm!("mfence", "lfence", options(nostack, preserves_flags));
                }

                #[cfg(all(target_arch = "aarch64", not(target_os = "macos")))]
                unsafe {
                    let end = self.ptr as usize + self.size;
                    extern "C" {
                        fn __clear_cache(start: *mut c_void, end: *mut c_void);
                    }
                    __clear_cache(self.ptr as *mut c_void, end as *mut c_void);
                }
            }
        }

        self.executable = true;
        Ok(())
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }

    pub fn size(&self) -> usize {
        self.size
    }
}

impl Drop for JitBuffer {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        {
            unsafe {
                sys::VirtualFree(self.ptr as *mut c_void, 0, sys::MEM_RELEASE);
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            unsafe {
                sys::munmap(self.ptr as *mut c_void, self.size);
            }
        }
    }
}
