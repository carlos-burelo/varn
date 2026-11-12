use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::super::capture::Shared;

static ACTIVE: AtomicPtr<Shared> = AtomicPtr::new(std::ptr::null_mut());

extern "C" fn on_signal(_: libc::c_int, _: *mut libc::siginfo_t, context: *mut libc::c_void) {
    let shared = ACTIVE.load(Ordering::Acquire);
    if !shared.is_null() {
        unsafe { (*shared).capture(program_counter(context), 0, thread_cpu_nanos()) };
    }
}

fn thread_cpu_nanos() -> u64 {
    let mut now: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) };
    now.tv_sec as u64 * 1_000_000_000 + now.tv_nsec as u64
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn program_counter(context: *mut libc::c_void) -> usize {
    let context = context as *const libc::ucontext_t;
    unsafe { (*context).uc_mcontext.gregs[libc::REG_RIP as usize] as usize }
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
fn program_counter(context: *mut libc::c_void) -> usize {
    let context = context as *const libc::ucontext_t;
    unsafe { (*context).uc_mcontext.pc as usize }
}

#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
fn program_counter(_: *mut libc::c_void) -> usize {
    0
}

struct Target(libc::pthread_t);

unsafe impl Send for Target {}

pub(in super::super) struct Sampler {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    previous: libc::sigaction,
}

impl Sampler {
    pub(in super::super) fn start(shared: &Shared, period: Duration) -> Result<Self, String> {
        let claimed = ACTIVE.compare_exchange(
            std::ptr::null_mut(),
            shared as *const Shared as *mut Shared,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if claimed.is_err() {
            return Err("another cpu profile is already running".to_owned());
        }
        let mut handler: libc::sigaction = unsafe { std::mem::zeroed() };
        handler.sa_sigaction = on_signal
            as extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void)
            as usize;
        handler.sa_flags = libc::SA_RESTART | libc::SA_SIGINFO;
        unsafe { libc::sigemptyset(&mut handler.sa_mask) };
        let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
        if unsafe { libc::sigaction(libc::SIGPROF, &handler, &mut previous) } != 0 {
            ACTIVE.store(std::ptr::null_mut(), Ordering::Release);
            return Err("could not install the sampling signal handler".to_owned());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let target = Target(unsafe { libc::pthread_self() });
        let thread = std::thread::Builder::new()
            .name("vn-cpu-profiler".to_owned())
            .spawn(move || {
                let target = target;
                run(&target, period, &flag);
            })
            .map_err(|e| format!("could not start the sampling thread: {e}"))?;
        Ok(Self {
            stop,
            thread: Some(thread),
            previous,
        })
    }

    pub(in super::super) fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        unsafe { libc::sigaction(libc::SIGPROF, &self.previous, std::ptr::null_mut()) };
        ACTIVE.store(std::ptr::null_mut(), Ordering::Release);
    }
}

fn run(target: &Target, period: Duration, stop: &AtomicBool) {
    let mut deadline = Instant::now() + period;
    while !stop.load(Ordering::Acquire) {
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline - now);
        }
        deadline = (deadline + period).max(Instant::now());
        if stop.load(Ordering::Acquire) {
            break;
        }
        unsafe { libc::pthread_kill(target.0, libc::SIGPROF) };
    }
}
