use super::sys;
use std::ffi::c_void;
use std::ptr;

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
