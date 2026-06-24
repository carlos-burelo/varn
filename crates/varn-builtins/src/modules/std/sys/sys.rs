use std::sync::OnceLock;
use std::time::Instant;
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct SysRuntime;

static START_TIME: OnceLock<Instant> = OnceLock::new();

varn_contract! {
    module: "runtime:process",
    contract: "src/modules/std/sys/runtime/sys_runtime.vn",
    impl SysRuntime {
        fn processPlatform(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(std::env::consts::OS.to_string())
        }
        fn processCwd(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default())
        }
        fn processArgs(ctx: &mut dyn NativeCtx) -> Result<Vec<VmValue>, String> {
            Ok(std::env::args().map(|s| ctx.alloc_str_owned(s)).collect())
        }
        fn processExit(_ctx: &mut dyn NativeCtx, code: Option<i64>) -> Result<(), String> {
            std::process::exit(code.unwrap_or(0) as i32)
        }
        fn processEnv(_ctx: &mut dyn NativeCtx, key: &str) -> Result<String, String> {
            Ok(std::env::var(key).unwrap_or_default())
        }
        fn processSetEnv(_ctx: &mut dyn NativeCtx, _key: &str, _val: &str) -> Result<(), String> {

            Ok(())
        }
        fn processNow(_ctx: &mut dyn NativeCtx) -> Result<f64, String> {
            let start = START_TIME.get_or_init(Instant::now);
            Ok(start.elapsed().as_secs_f64() * 1000.0)
        }
    }
}
