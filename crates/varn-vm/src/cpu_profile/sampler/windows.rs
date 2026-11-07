use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::super::capture::Shared;

type Handle = isize;

const THREAD_SUSPEND_RESUME: u32 = 0x0002;
const THREAD_GET_CONTEXT: u32 = 0x0008;
const THREAD_QUERY_LIMITED_INFORMATION: u32 = 0x0800;
const CONTEXT_CONTROL: u32 = 0x0010_0001;
const CONTEXT_FLAGS_OFFSET: usize = 0x30;
const CONTEXT_RIP_OFFSET: usize = 0xF8;
const CONTEXT_RSP_OFFSET: usize = 0x98;
const CONTEXT_BYTES: usize = 1232;

#[repr(C, align(16))]
struct Context([u8; CONTEXT_BYTES]);

unsafe extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn GetCurrentThread() -> Handle;
    fn DuplicateHandle(
        source_process: Handle,
        source: Handle,
        target_process: Handle,
        target: *mut Handle,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn SuspendThread(thread: Handle) -> u32;
    fn ResumeThread(thread: Handle) -> u32;
    fn GetThreadContext(thread: Handle, context: *mut Context) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn QueryThreadCycleTime(thread: Handle, cycles: *mut u64) -> i32;
}

struct Target(Handle);

unsafe impl Send for Target {}

struct SharedRef(*const Shared);

unsafe impl Send for SharedRef {}

pub(in super::super) struct Sampler {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Target>>,
}

impl Sampler {
    pub(in super::super) fn start(shared: &Shared, period: Duration) -> Result<Self, String> {
        let mut target: Handle = 0;
        let duplicated = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                GetCurrentThread(),
                GetCurrentProcess(),
                &mut target,
                THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_LIMITED_INFORMATION,
                0,
                0,
            )
        };
        if duplicated == 0 {
            return Err("could not open a handle to the executing thread".to_owned());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let target = Target(target);
        let shared = SharedRef(shared);
        let thread = std::thread::Builder::new()
            .name("vn-cpu-profiler".to_owned())
            .spawn(move || {
                let (target, shared) = (target, shared);
                run(&target, unsafe { &*shared.0 }, period, &flag);
                target
            })
            .map_err(|e| format!("could not start the sampling thread: {e}"))?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    pub(in super::super) fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && let Ok(target) = thread.join()
        {
            unsafe { CloseHandle(target.0) };
        }
    }
}

fn run(target: &Target, shared: &Shared, period: Duration, stop: &AtomicBool) {
    let mut context = Context([0; CONTEXT_BYTES]);
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
        if unsafe { SuspendThread(target.0) } == u32::MAX {
            continue;
        }
        context.0[CONTEXT_FLAGS_OFFSET..CONTEXT_FLAGS_OFFSET + 4]
            .copy_from_slice(&CONTEXT_CONTROL.to_le_bytes());
        if unsafe { GetThreadContext(target.0, &mut context) } != 0 {
            let mut rip = [0u8; 8];
            rip.copy_from_slice(&context.0[CONTEXT_RIP_OFFSET..CONTEXT_RIP_OFFSET + 8]);
            let mut cycles = 0u64;
            unsafe { QueryThreadCycleTime(target.0, &mut cycles) };
            let mut rsp = [0u8; 8];
            rsp.copy_from_slice(&context.0[CONTEXT_RSP_OFFSET..CONTEXT_RSP_OFFSET + 8]);
            let stack = u64::from_le_bytes(rsp) as usize;
            let caller = if stack != 0 && stack.is_multiple_of(8) {
                unsafe { *(stack as *const usize) }
            } else {
                0
            };
            unsafe { shared.capture(u64::from_le_bytes(rip) as usize, caller, cycles) };
        }
        unsafe { ResumeThread(target.0) };
    }
}
