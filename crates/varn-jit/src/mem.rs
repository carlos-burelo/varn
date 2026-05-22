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

        pub fn VirtualFree(
            lpAddress: *mut c_void,
            dwSize: usize,
            dwFreeType: u32,
        ) -> i32;

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
    pub const MAP_ANONYMOUS: i32 = 0x20;
    pub const MAP_FAILED: *mut c_void = !0 as *mut c_void;

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
}

/// A safe wrapper around dynamically allocated executable memory pages.
pub struct JitBuffer {
    ptr: *mut u8,
    size: usize,
    executable: bool,
}

impl JitBuffer {
    /// Allocates a new Read-Write (RW) buffer of a given size.
    pub fn new(size: usize) -> Result<Self, String> {
        if size == 0 {
            return Err("Size must be greater than zero".to_owned());
        }

        // Round up to system page size (typically 4096 bytes)
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
            let ptr = unsafe {
                sys::mmap(
                    ptr::null_mut(),
                    size,
                    sys::PROT_READ | sys::PROT_WRITE,
                    sys::MAP_PRIVATE | sys::MAP_ANONYMOUS,
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

    /// Returns a slice of the buffer for writing. Only valid if not yet marked executable.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        assert!(!self.executable, "Cannot modify an executable JIT buffer");
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.size) }
    }

    /// Makes the buffer executable (RX) and flushes the CPU instruction cache.
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
                return Err("Failed to change memory protection to executable (PAGE_EXECUTE_READ)".to_owned());
            }

            // Flush the instruction cache to ensure CPU sees our newly written bytes
            unsafe {
                sys::FlushInstructionCache(sys::GetCurrentProcess(), self.ptr as *const c_void, self.size);
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let res = unsafe {
                sys::mprotect(
                    self.ptr as *mut c_void,
                    self.size,
                    sys::PROT_READ | sys::PROT_EXEC,
                )
            };
            if res != 0 {
                return Err("Failed to change memory protection to executable (mprotect)".to_owned());
            }

            // Flush CPU instruction cache
            #[cfg(target_arch = "x86_64")]
            {
                // x86_64 has hardware-managed instruction cache coherence,
                // but we flush caches or execute a fence to be strictly correct.
                unsafe {
                    std::arch::asm!("mfence", "lfence", options(nostack, preserves_flags));
                }
            }

            #[cfg(target_arch = "aarch64")]
            {
                // AArch64 requires explicit cache flushing.
                unsafe {
                    let start = self.ptr as usize;
                    let end = start + self.size;
                    // Clear data cache, then instruction cache, then sync
                    // Note: In pure Rust we can call __builtin___clear_cache via GCC/Clang builtins
                    // but on ARM64 we can also use asm.
                    // For now, let's use the clear_cache builtin if compiling with standard linkers.
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

    /// Returns the raw pointer to the allocated memory.
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }

    /// Returns the size of the allocated memory.
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
