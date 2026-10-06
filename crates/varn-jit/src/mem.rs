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











pub const STACK_RESERVE_BYTES: usize = 64 << 20;


pub struct StackArenas {
    gpr: JitBuffer,
    fpr: JitBuffer,
    refs: JitBuffer,
    dyn_: JitBuffer,
    top_gpr: u32,
    top_fpr: u32,
    top_ref: u32,
    top_dyn: u32,
}

impl StackArenas {
    
    pub fn new(bytes: usize) -> Result<Self, String> {
        Ok(Self {
            gpr: JitBuffer::new(bytes)?,
            fpr: JitBuffer::new(bytes)?,
            refs: JitBuffer::new(bytes)?,
            dyn_: JitBuffer::new(bytes)?,
            top_gpr: 0,
            top_fpr: 0,
            top_ref: 0,
            top_dyn: 0,
        })
    }

    
    pub fn abi_stacks(&self) -> varn_abi::AbiStacks {
        let gpr = self.gpr.as_ptr() as *mut i64;
        let fpr = self.fpr.as_ptr() as *mut f64;
        let refs = self.refs.as_ptr() as *mut u32;
        let dyn_ = self.dyn_.as_ptr() as *mut varn_abi::AbiValue;
        
        unsafe {
            varn_abi::AbiStacks {
                gpr,
                gpr_end: gpr.add(self.gpr.size() / size_of::<i64>()),
                fpr,
                fpr_end: fpr.add(self.fpr.size() / size_of::<f64>()),
                refs,
                refs_end: refs.add(self.refs.size() / size_of::<u32>()),
                dyn_,
                dyn_end: dyn_.add(self.dyn_.size() / size_of::<varn_abi::AbiValue>()),
            }
        }
    }

    
    
    pub fn alloc(&mut self, counts: [u32; 4]) -> Option<varn_abi::ActBases> {
        let cap = |bytes: usize, elem: usize| (bytes / elem) as u64;
        let ok = (self.top_gpr as u64 + counts[0] as u64) <= cap(self.gpr.size(), 8)
            && (self.top_fpr as u64 + counts[1] as u64) <= cap(self.fpr.size(), 8)
            && (self.top_ref as u64 + counts[2] as u64) <= cap(self.refs.size(), 4)
            && (self.top_dyn as u64 + counts[3] as u64) <= cap(self.dyn_.size(), 16);
        if !ok {
            return None;
        }
        let bases = varn_abi::ActBases {
            bases: [self.top_gpr, self.top_fpr, self.top_ref, self.top_dyn],
        };
        self.top_gpr += counts[0];
        self.top_fpr += counts[1];
        self.top_ref += counts[2];
        self.top_dyn += counts[3];
        Some(bases)
    }

    
    pub fn truncate(&mut self, bases: varn_abi::ActBases) {
        self.top_gpr = bases.bases[0];
        self.top_fpr = bases.bases[1];
        self.top_ref = bases.bases[2];
        self.top_dyn = bases.bases[3];
    }

    pub fn reset(&mut self) {
        self.top_gpr = 0;
        self.top_fpr = 0;
        self.top_ref = 0;
        self.top_dyn = 0;
    }
}

use core::mem::size_of;


pub struct FrameArena {
    buf: JitBuffer,
    len: u32,
}

impl FrameArena {
    pub fn new(cap_frames: u32) -> Result<Self, String> {
        let bytes = (cap_frames as usize).saturating_mul(size_of::<varn_abi::AbiFrame>());
        Ok(Self {
            buf: JitBuffer::new(bytes.max(4096))?,
            len: 0,
        })
    }

    pub fn cap(&self) -> u32 {
        (self.buf.size() / size_of::<varn_abi::AbiFrame>()) as u32
    }

    
    pub fn push(&mut self, frame: varn_abi::AbiFrame) -> Option<u32> {
        if self.len >= self.cap() {
            return None;
        }
        let id = self.len;
        
        unsafe {
            let base = self.buf.as_ptr() as *mut varn_abi::AbiFrame;
            base.add(id as usize).write(frame);
        }
        self.len += 1;
        Some(id)
    }

    pub fn pop(&mut self) {
        self.len = self.len.saturating_sub(1);
    }

    pub fn abi(&self) -> varn_abi::AbiFrameArena {
        varn_abi::AbiFrameArena {
            base: self.buf.as_ptr() as *mut varn_abi::AbiFrame,
            len: self.len,
            cap: self.cap(),
        }
    }
}

#[cfg(test)]
mod abi_v2_tests {
    use super::*;

    #[test]
    fn bases_estables_tras_alloc() {
        let mut a = StackArenas::new(1 << 20).unwrap();
        let b0 = a.abi_stacks();
        a.alloc([10, 5, 5, 5]).unwrap();
        let b1 = a.abi_stacks();
        assert_eq!(b0.gpr, b1.gpr);
        assert_eq!(b0.fpr, b1.fpr);
        assert_eq!(b0.refs, b1.refs);
        assert_eq!(b0.dyn_ as *const u8, b1.dyn_ as *const u8);
    }

    #[test]
    fn overflow_rechaza_slow_path() {
        let mut a = StackArenas::new(4096).unwrap();
        assert!(a.alloc([u32::MAX, 0, 0, 0]).is_none());
    }

    #[test]
    fn frame_arena_push_pop() {
        let mut f = FrameArena::new(4).unwrap();
        let frame = varn_abi::AbiFrame {
            closure: core::ptr::null(),
            caller: 0,
            resume: 0,
            dest: 0,
            _pad: 0,
            bases: varn_abi::ActBases { bases: [0; 4] },
        };
        assert_eq!(f.push(frame), Some(0));
        assert_eq!(f.push(frame), Some(1));
        f.pop();
        assert_eq!(f.abi().len, 1);
    }
}
