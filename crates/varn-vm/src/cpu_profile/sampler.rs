#[cfg(unix)]
mod unix;
#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;

#[cfg(unix)]
pub(super) use unix::Sampler;
#[cfg(all(windows, target_arch = "x86_64"))]
pub(super) use windows::Sampler;

#[cfg(not(any(unix, all(windows, target_arch = "x86_64"))))]
mod unsupported;
#[cfg(not(any(unix, all(windows, target_arch = "x86_64"))))]
pub(super) use unsupported::Sampler;
