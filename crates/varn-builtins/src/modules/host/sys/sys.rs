use std::sync::OnceLock;
use std::time::Instant;
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct SysRuntime;

static START_TIME: OnceLock<Instant> = OnceLock::new();

varn_contract! {
    module: "runtime:sys",
    contract: "src/modules/host/sys/sys_runtime.vn",
    impl SysRuntime {
        fn platform(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(std::env::consts::OS.to_string())
        }
        fn cwd(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default())
        }
        fn args(ctx: &mut dyn NativeCtx) -> Result<Vec<VmValue>, String> {
            Ok(std::env::args().map(|s| ctx.alloc_str_owned(s)).collect())
        }
        fn exit(_ctx: &mut dyn NativeCtx, code: Option<i64>) -> Result<(), String> {
            std::process::exit(code.unwrap_or(0) as i32)
        }
        fn env(_ctx: &mut dyn NativeCtx, key: &str) -> Result<String, String> {
            Ok(std::env::var(key).unwrap_or_default())
        }
        fn setEnv(_ctx: &mut dyn NativeCtx, _key: &str, _val: &str) -> Result<(), String> {

            Ok(())
        }
        fn now(_ctx: &mut dyn NativeCtx) -> Result<f64, String> {
            let start = START_TIME.get_or_init(Instant::now);
            Ok(start.elapsed().as_secs_f64() * 1000.0)
        }
    }
}
