//! Host architecture abstraction layer.
//!
//! Encapsulates platform- and architecture-specific low-level primitives:
//! stack jump buffers (`JmpBuf`, `vm_setjmp`, `vm_longjmp`) for JIT panic recovery
//! and async coroutine suspension.

#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
mod x86_64_windows;
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
pub use x86_64_windows::*;

#[cfg(all(target_arch = "x86_64", not(target_os = "windows")))]
mod x86_64_sysv;
#[cfg(all(target_arch = "x86_64", not(target_os = "windows")))]
pub use x86_64_sysv::*;

#[cfg(target_arch = "aarch64")]
mod aarch64;
#[cfg(target_arch = "aarch64")]
pub use aarch64::*;

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod fallback;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub use fallback::*;
