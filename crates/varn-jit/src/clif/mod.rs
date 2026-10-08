pub mod abi;
pub(crate) mod alloc;
pub mod debug;
pub(crate) mod emit;
pub(crate) mod fields;
pub(crate) mod floats;
pub(crate) mod from_ssa;
pub(crate) mod generic;
pub(crate) mod homes;
pub mod lower;
pub(crate) mod native_abi;
pub(crate) mod piece;
pub(crate) mod strings;

use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::Function;
use cranelift_codegen::isa::{OwnedTargetIsa, TargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("VARN_NO_CLIF").is_err())
}

pub fn trace() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("VARN_CLIF_TRACE").is_ok())
}

pub fn shared_isa() -> Result<&'static OwnedTargetIsa, String> {
    static ISA: OnceLock<Result<OwnedTargetIsa, String>> = OnceLock::new();
    ISA.get_or_init(host_isa).as_ref().map_err(|e| e.clone())
}

pub fn host_isa() -> Result<OwnedTargetIsa, String> {
    let mut flags = settings::builder();
    let opt = std::env::var("VARN_CLIF_OPT").unwrap_or_else(|_| "speed".to_owned());
    flags.set("opt_level", &opt).map_err(|e| e.to_string())?;

    let verify = std::env::var("VARN_CLIF_VERIFY").is_ok() || cfg!(debug_assertions);
    flags
        .set("enable_verifier", if verify { "true" } else { "false" })
        .map_err(|e| e.to_string())?;
    flags
        .set("preserve_frame_pointers", "true")
        .map_err(|e| e.to_string())?;
    let isa_builder = cranelift_native::builder_with_options(true).map_err(|e| e.to_string())?;
    isa_builder
        .finish(settings::Flags::new(flags))
        .map_err(|e| e.to_string())
}

pub(crate) fn with_ctx<R>(
    func: Function,
    isa: &dyn TargetIsa,
    take: impl FnOnce(&cranelift_codegen::CompiledCode) -> Result<R, String>,
) -> Result<R, String> {
    let mut ctx = Context::for_function(func);
    compile_in(&mut ctx, isa, take)
}

fn compile_in<R>(
    ctx: &mut Context,
    isa: &dyn TargetIsa,
    take: impl FnOnce(&cranelift_codegen::CompiledCode) -> Result<R, String>,
) -> Result<R, String> {
    if std::env::var("VARN_CLIF_PRINT_IR").is_ok() {
        eprintln!("=== CLIF IR ===\n{}", ctx.func.display());
    }
    let start = std::time::Instant::now();
    let mut take_opt = Some(take);
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let compiled = ctx.compile(isa, &mut ControlPlane::default());
        match compiled {
            Ok(c) => {
                let take_fn = take_opt.take().unwrap();
                take_fn(c)
            }
            Err(e) => Err(format!("clif compile: {e:?}")),
        }
    }));
    crate::stats::JIT_STATS
        .backend_time_ns
        .fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
    match res {
        Ok(r) => r,
        Err(_) => Err("clif backend compile panicked".to_string()),
    }
}
