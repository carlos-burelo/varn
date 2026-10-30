use std::sync::OnceLock;
use std::time::Instant;
use varn_core::StdErrorClass;
use varn_macros::varn_contract;
use varn_types::NativeError;
use varn_types::{NativeCtx, VmValue};

pub struct SysRuntime;

static START_TIME: OnceLock<Instant> = OnceLock::new();

const FIRST_READ_OFFSET_MS: u64 = 1;

fn clock_origin() -> Instant {
    Instant::now()
        .checked_sub(std::time::Duration::from_millis(FIRST_READ_OFFSET_MS))
        .unwrap_or_else(Instant::now)
}

varn_contract! {
    module: "runtime:sys",
    contract: "src/modules/runtime/sys/sys_runtime.vn",
    impl SysRuntime {
        fn platform(_ctx: &mut dyn NativeCtx) -> Result<String, NativeError> {
            Ok(std::env::consts::OS.to_string())
        }
        fn cwd(_ctx: &mut dyn NativeCtx) -> Result<String, NativeError> {
            (std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .map_err(|e| format!("sys.cwd: cannot read the current directory: {e}"))).map_err(|e| NativeError::std(StdErrorClass::Io, e))
        }
        fn args(ctx: &mut dyn NativeCtx) -> Result<Vec<VmValue>, NativeError> {
            Ok(std::env::args().map(|s| ctx.alloc_str_owned(s)).collect())
        }
        fn exit(_ctx: &mut dyn NativeCtx, code: Option<i64>) -> Result<(), NativeError> {
            std::process::exit(code.unwrap_or(0) as i32)
        }
        fn env(ctx: &mut dyn NativeCtx, key: &str) -> Result<Option<String>, NativeError> {
            if !ctx.check_env(key) {
                return Err(NativeError::capability("sys.env", key));
            }
            match std::env::var(key) {
                Ok(value) => Ok(Some(value)),
                Err(std::env::VarError::NotPresent) => Ok(None),
                Err(std::env::VarError::NotUnicode(_)) => {
                    Err(NativeError::std(StdErrorClass::Parse, format!("sys.env: variable '{key}' is not valid Unicode")))
                }
            }
        }
        fn setEnv(ctx: &mut dyn NativeCtx, key: &str, val: &str) -> Result<(), NativeError> {
            if !ctx.check_env(key) {
                return Err(NativeError::capability("sys.env", key));
            }
            unsafe { std::env::set_var(key, val) };
            Ok(())
        }
        fn now(_ctx: &mut dyn NativeCtx) -> Result<f64, NativeError> {
            let start = START_TIME.get_or_init(clock_origin);
            Ok(start.elapsed().as_secs_f64() * 1000.0)
        }
    }
}
