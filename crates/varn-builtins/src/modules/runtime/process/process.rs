use parking_lot::{Mutex, RwLock};
use rustc_hash::FxHashMap;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use varn_macros::varn_contract;
use varn_types::value::SendValue;
use varn_types::{NativeCtx, VmValue, VnArray};

use crate::runtime::blocking;

static PROCESS_POOL: RwLock<Vec<Option<Child>>> = RwLock::new(Vec::new());
static EXEC_RESULTS: Mutex<Option<FxHashMap<i64, ExecResult>>> = Mutex::new(None);
static NEXT_EXEC: AtomicI64 = AtomicI64::new(1);

const WAIT_POLL_START: Duration = Duration::from_millis(1);
const WAIT_POLL_MAX: Duration = Duration::from_millis(20);

struct ExecResult {
    exit_code: i64,
    success: bool,
    stdout: String,
    stderr: String,
}

fn run_shell(command: &str) -> Result<ExecResult, String> {
    #[cfg(windows)]
    let output = Command::new("cmd").args(["/C", command]).output();

    #[cfg(not(windows))]
    let output = Command::new("sh").args(["-c", command]).output();

    let output = output.map_err(|e| format!("Failed to execute process: {e}"))?;
    Ok(ExecResult {
        exit_code: output.status.code().unwrap_or(-1) as i64,
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn store_result(result: ExecResult) -> i64 {
    let id = NEXT_EXEC.fetch_add(1, Ordering::Relaxed);
    EXEC_RESULTS
        .lock()
        .get_or_insert_with(FxHashMap::default)
        .insert(id, result);
    id
}

fn take_result(id: i64) -> Option<ExecResult> {
    EXEC_RESULTS.lock().as_mut()?.remove(&id)
}

fn wait_child(pid: i64) -> Result<i64, String> {
    let index = usize::try_from(pid).map_err(|_| "Invalid process handle".to_string())?;
    let mut delay = WAIT_POLL_START;
    loop {
        {
            let mut pool = PROCESS_POOL.write();
            let slot = pool
                .get_mut(index)
                .ok_or_else(|| format!("Process handle {pid} not found"))?;
            let child = slot
                .as_mut()
                .ok_or_else(|| format!("Process {pid} already waited, killed or exited"))?;
            let waited = child
                .try_wait()
                .map_err(|e| format!("Failed to wait process {pid}: {e}"))?;
            if let Some(status) = waited {
                *slot = None;
                return Ok(status.code().unwrap_or(-1) as i64);
            }
        }
        std::thread::sleep(delay);
        delay = (delay * 2).min(WAIT_POLL_MAX);
    }
}

pub struct ProcessRuntime;

varn_macros::varn_record! {
    contract: "src/modules/runtime/process/process_runtime.vn",
    interface: "ProcessOutput",
    struct ProcessOutput;
}

varn_macros::varn_record! {
    contract: "src/modules/runtime/process/process_runtime.vn",
    interface: "ProcessStatus",
    struct ProcessStatus;
}

fn output_record(ctx: &mut dyn NativeCtx, result: ExecResult) -> VmValue {
    let stdout = ctx.alloc_str_owned(result.stdout);
    let stderr = ctx.alloc_str_owned(result.stderr);
    ProcessOutput {
        exit_code: result.exit_code,
        stdout,
        stderr,
        success: result.success,
    }
    .alloc(ctx)
}

varn_contract! {
    module: "runtime:process",
    contract: "src/modules/runtime/process/process_runtime.vn",
    impl ProcessRuntime {
        fn exec(ctx: &mut dyn NativeCtx, command: &str) -> Result<VmValue, String> {
            let result = run_shell(command)?;
            Ok(output_record(ctx, result))
        }

        fn execAsync(ctx: &mut dyn NativeCtx, command: &str) -> Result<VmValue, String> {
            let command = command.to_owned();
            Ok(blocking::task(ctx, move || {
                run_shell(&command).map(|result| SendValue::Int(store_result(result)))
            }))
        }

        fn execResult(ctx: &mut dyn NativeCtx, id: i64) -> Result<VmValue, String> {
            let result = take_result(id).ok_or_else(|| format!("No exec result {id}"))?;
            Ok(output_record(ctx, result))
        }

        fn spawnProcess(ctx: &mut dyn NativeCtx, command: &str, args: VnArray) -> Result<i64, String> {
            let len = args.len(ctx);
            let mut string_args = Vec::with_capacity(len);
            for i in 0..len {
                let v = args.get(ctx, i).unwrap_or(VmValue::null());
                if let Some(s) = ctx.str_owned(v) {
                    string_args.push(s);
                }
            }
            let child = Command::new(command)
                .args(&string_args)
                .spawn()
                .map_err(|e| format!("Failed to spawn process '{command}': {e}"))?;

            let mut pool = PROCESS_POOL.write();
            let id = pool.len() as i64;
            pool.push(Some(child));
            Ok(id)
        }

        fn waitProcess(ctx: &mut dyn NativeCtx, pid: i64) -> Result<VmValue, String> {
            let exit_code = wait_child(pid)?;
            Ok(ProcessStatus { exit_code, success: exit_code == 0 }.alloc(ctx))
        }

        fn waitProcessAsync(ctx: &mut dyn NativeCtx, pid: i64) -> Result<VmValue, String> {
            Ok(blocking::task(ctx, move || wait_child(pid).map(SendValue::Int)))
        }

        fn currentExe(ctx: &mut dyn NativeCtx) -> Result<VmValue, String> {
            let path = std::env::current_exe()
                .map_err(|e| format!("Failed to resolve current executable: {e}"))?;
            Ok(ctx.alloc_str_owned(path.to_string_lossy().into_owned()))
        }

        fn killProcess(_ctx: &mut dyn NativeCtx, pid: i64) -> Result<bool, String> {
            if pid < 0 {
                return Err("Invalid process handle".to_string());
            }

            let mut pool = PROCESS_POOL.write();
            let slot = pool.get_mut(pid as usize)
                .ok_or_else(|| format!("Process handle {pid} not found"))?;

            if let Some(mut child) = slot.take() {
                let _ = child.kill();
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }
}
