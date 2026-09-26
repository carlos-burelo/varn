//! Self-reported process memory.
//!
//! This exists because there was no way to answer "how much RAM is this
//! server using right now, and why" from inside the process itself — every
//! investigation had to shell out to `tasklist`/`ps` from outside. Reads the
//! OS's own bookkeeping for this process (`GetProcessMemoryInfo` on Windows,
//! `/proc/self/status` on Linux) — the same number Task Manager shows, not a
//! self-estimate the allocator could get wrong.

/// Resident set size of the current process, in KB. `None` on a platform this
/// doesn't cover yet (anything but Windows/Linux) or if the OS call fails.
pub fn resident_kb() -> Option<u64> {
    imp::resident_kb()
}

#[cfg(target_os = "windows")]
mod imp {
    // A minimal, hand-written binding for one WinAPI call rather than a new
    // dependency on the full `windows-sys` crate for it: `psapi.dll` and its
    // ABI have been stable since Windows XP, and pulling in a multi-thousand-
    // item binding crate for a single struct and two functions is the kind of
    // dependency weight this workspace's other crates go out of their way to
    // avoid (see e.g. `varn-mcp`'s own binary-resolution code, which does the
    // same "write the ten lines instead of the dependency" call).
    #[repr(C)]
    #[allow(non_snake_case)]
    struct ProcessMemoryCounters {
        cb: u32,
        PageFaultCount: u32,
        PeakWorkingSetSize: usize,
        WorkingSetSize: usize,
        QuotaPeakPagedPoolUsage: usize,
        QuotaPagedPoolUsage: usize,
        QuotaPeakNonPagedPoolUsage: usize,
        QuotaNonPagedPoolUsage: usize,
        PagefileUsage: usize,
        PeakPagefileUsage: usize,
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut core::ffi::c_void,
            counters: *mut ProcessMemoryCounters,
            size: u32,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
    }

    pub fn resident_kb() -> Option<u64> {
        let mut counters: ProcessMemoryCounters = unsafe { std::mem::zeroed() };
        counters.cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;
        // SAFETY: `GetCurrentProcess` returns a pseudo-handle that needs no
        // closing; `counters` is sized and zeroed to exactly what the ABI
        // expects, and `cb` tells the call its own size, which is how this
        // API validates the buffer it was handed.
        let ok = unsafe {
            GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb)
        };
        if ok == 0 {
            None
        } else {
            Some((counters.WorkingSetSize / 1024) as u64)
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    pub fn resident_kb() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                return digits.parse().ok();
            }
        }
        None
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
mod imp {
    pub fn resident_kb() -> Option<u64> {
        None
    }
}
