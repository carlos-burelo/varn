use parking_lot::RwLock;
use std::process::{Child, Command};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, Value, VmValue, VnArray};

static PROCESS_POOL: RwLock<Vec<Option<Child>>> = RwLock::new(Vec::new());

pub struct ProcessRuntime;

varn_contract! {
    module: "runtime:process",
    contract: "src/modules/host/process/process_runtime.vn",
    impl ProcessRuntime {
        fn exec(ctx: &mut dyn NativeCtx, command: &str) -> Result<VmValue, String> {
            #[cfg(windows)]
            let output = Command::new("cmd")
                .args(["/C", command])
                .output()
                .map_err(|e| format!("Failed to execute process: {e}"))?;

            #[cfg(not(windows))]
            let output = Command::new("sh")
                .args(["-c", command])
                .output()
                .map_err(|e| format!("Failed to execute process: {e}"))?;

            let exit_code = output.status.code().unwrap_or(-1) as i64;
            let success = output.status.success();
            let stdout_str = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr_str = String::from_utf8_lossy(&output.stderr).into_owned();

            let obj = ctx.alloc_object();
            ctx.set_field(obj, "exitCode", VmValue::from_int(exit_code));
            ctx.set_field(obj, "success", VmValue::from_bool(success));

            let out_nv = ctx.intern(Value::Str(stdout_str.into()));
            ctx.set_field(obj, "stdout", out_nv);

            let err_nv = ctx.intern(Value::Str(stderr_str.into()));
            ctx.set_field(obj, "stderr", err_nv);

            Ok(obj)
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
            if pid < 0 {
                return Err("Invalid process handle".to_string());
            }

            let mut pool = PROCESS_POOL.write();
            let slot = pool.get_mut(pid as usize)
                .ok_or_else(|| format!("Process handle {pid} not found"))?;

            let mut child = slot.take()
                .ok_or_else(|| format!("Process {pid} already waited or exited"))?;

            let status = child.wait().map_err(|e| format!("Failed to wait process {pid}: {e}"))?;
            let exit_code = status.code().unwrap_or(-1) as i64;
            let success = status.success();

            let obj = ctx.alloc_object();
            ctx.set_field(obj, "exitCode", VmValue::from_int(exit_code));
            ctx.set_field(obj, "success", VmValue::from_bool(success));
            Ok(obj)
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
