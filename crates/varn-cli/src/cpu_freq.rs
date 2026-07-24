//! Best-effort current CPU frequency, sampled during a benchmark run so the
//! output can show whether the CPU was turboing or throttled to base. There is
//! deliberately **no temperature**: on Windows the ACPI thermal zone is not
//! exposed without a kernel driver, so a portable, dependency-free reading is
//! not possible. Frequency, however, already reveals throttling (it collapses
//! toward the base clock under sustained load).

/// A frequency sample in MHz. `cur_mhz` is the (peak) current clock; `max_mhz`
/// is the processor's rated/base clock as the OS reports it (on Intel this is
/// typically the *base*, so `cur_mhz` can exceed it while turboing).
#[derive(Clone, Copy)]
pub struct CpuFreq {
    pub cur_mhz: u32,
    pub max_mhz: u32,
}

#[cfg(windows)]
pub fn sample() -> Option<CpuFreq> {
    // PROCESSOR_POWER_INFORMATION, one entry per logical processor.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Ppi {
        number: u32,
        max_mhz: u32,
        current_mhz: u32,
        mhz_limit: u32,
        max_idle_state: u32,
        current_idle_state: u32,
    }
    // ProcessorInformation = 11.
    #[link(name = "powrprof")]
    extern "system" {
        fn CallNtPowerInformation(
            level: i32,
            in_buf: *mut core::ffi::c_void,
            in_len: u32,
            out_buf: *mut core::ffi::c_void,
            out_len: u32,
        ) -> i32;
    }

    let n = std::thread::available_parallelism()
        .map(|x| x.get())
        .unwrap_or(1);
    let mut v = vec![
        Ppi {
            number: 0,
            max_mhz: 0,
            current_mhz: 0,
            mhz_limit: 0,
            max_idle_state: 0,
            current_idle_state: 0,
        };
        n
    ];
    let out_len = (core::mem::size_of::<Ppi>() * n) as u32;
    // Returns STATUS_SUCCESS (0) on success.
    let status = unsafe {
        CallNtPowerInformation(
            11,
            core::ptr::null_mut(),
            0,
            v.as_mut_ptr() as *mut core::ffi::c_void,
            out_len,
        )
    };
    if status != 0 {
        return None;
    }
    let cur = v.iter().map(|p| p.current_mhz).max().unwrap_or(0);
    let max = v.iter().map(|p| p.max_mhz).max().unwrap_or(0);
    if cur == 0 {
        None
    } else {
        Some(CpuFreq {
            cur_mhz: cur,
            max_mhz: max,
        })
    }
}

#[cfg(target_os = "linux")]
pub fn sample() -> Option<CpuFreq> {
    // Peak scaling_cur_freq across cores (kHz → MHz).
    let mut cur = 0u32;
    for i in 0.. {
        let p = format!("/sys/devices/system/cpu/cpu{i}/cpufreq/scaling_cur_freq");
        match std::fs::read_to_string(&p) {
            Ok(s) => {
                if let Ok(khz) = s.trim().parse::<u32>() {
                    cur = cur.max(khz / 1000);
                }
            }
            Err(_) => break,
        }
    }
    if cur == 0 {
        // Fallback: /proc/cpuinfo "cpu MHz".
        if let Ok(txt) = std::fs::read_to_string("/proc/cpuinfo") {
            for line in txt.lines() {
                if line.starts_with("cpu MHz") {
                    if let Some(val) = line.split(':').nth(1) {
                        if let Ok(f) = val.trim().parse::<f64>() {
                            cur = cur.max(f as u32);
                        }
                    }
                }
            }
        }
    }
    let max = std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(|khz| khz / 1000)
        .unwrap_or(0);
    if cur == 0 {
        None
    } else {
        Some(CpuFreq {
            cur_mhz: cur,
            max_mhz: max,
        })
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn sample() -> Option<CpuFreq> {
    None
}

/// Merge two samples keeping the higher current clock (the peak under load).
pub fn keep_peak(a: Option<CpuFreq>, b: Option<CpuFreq>) -> Option<CpuFreq> {
    match (a, b) {
        (Some(x), Some(y)) => Some(if y.cur_mhz > x.cur_mhz { y } else { x }),
        (Some(x), None) => Some(x),
        (None, b) => b,
    }
}
