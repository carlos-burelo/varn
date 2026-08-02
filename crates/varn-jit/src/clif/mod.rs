//! Cranelift backend seam (phase 5a spike).
//!
//! Proves the toolchain end-to-end on this host: build CLIF in memory,
//! compile it with the native ISA (regalloc2, isel, mid-end), copy the
//! code bytes into our own W^X `JitBuffer`, and execute. No
//! `cranelift-jit`/`cranelift-module` — Varn owns code memory.
//!
//! Frame pointers are forced on: the native-frame call protocol resolves
//! stack walks (errors, debugger, GC) through an RBP chain plus a
//! return-address side table.

pub(crate) mod abi;
pub(crate) mod alloc;
pub(crate) mod arrays;
pub(crate) mod body;
pub(crate) mod classes;
pub mod debug;
pub(crate) mod emit;
pub(crate) mod piece;
pub(crate) mod preheader;
pub(crate) mod scan;
pub(crate) mod strconcat;
pub(crate) mod vars;
pub(crate) mod fields;
pub(crate) mod floats;
pub(crate) mod generic;
pub(crate) mod globals;
pub(crate) mod kinds;
pub(crate) mod liveness;
pub(crate) mod methods;
pub(crate) mod osr;
pub mod lower;

use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::Function;
use cranelift_codegen::isa::{OwnedTargetIsa, TargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

/// Escape hatch while the backend matures: `VARN_NO_CLIF=1` routes
/// everything back through the template JIT. Read once per process.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("VARN_NO_CLIF").is_err())
}

/// `VARN_CLIF_TRACE=1` logs each route/bail decision with its reason.
pub fn trace() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("VARN_CLIF_TRACE").is_ok())
}

/// The host ISA is immutable for the process lifetime; build it once.
pub fn shared_isa() -> Result<&'static OwnedTargetIsa, String> {
    static ISA: OnceLock<Result<OwnedTargetIsa, String>> = OnceLock::new();
    ISA.get_or_init(host_isa).as_ref().map_err(|e| e.clone())
}

/// Host ISA configured for Varn: speed-optimized, frame pointers kept.
///
/// Two knobs are env-overridable because their right value is a measurement,
/// not a constant: `VARN_CLIF_OPT` (`none`|`speed`|`speed_and_size`) trades
/// compile time against code quality, and `VARN_CLIF_VERIFY=1` puts back the
/// IR verifier, which Cranelift enables by default and which we pay for on
/// every compile in a shipped binary.
pub fn host_isa() -> Result<OwnedTargetIsa, String> {
    let mut flags = settings::builder();
    let opt = std::env::var("VARN_CLIF_OPT").unwrap_or_else(|_| "speed".to_owned());
    flags
        .set("opt_level", &opt)
        .map_err(|e| e.to_string())?;
    // Default-on in Cranelift and meant for compiler development: it re-walks
    // the whole function at several points per compile. Our lowering is fixed
    // at build time, so shipping it means paying a debug check per run.
    let verify = std::env::var("VARN_CLIF_VERIFY").is_ok() || cfg!(debug_assertions);
    flags
        .set("enable_verifier", if verify { "true" } else { "false" })
        .map_err(|e| e.to_string())?;
    flags
        .set("preserve_frame_pointers", "true")
        .map_err(|e| e.to_string())?;
    let isa_builder = cranelift_native::builder().map_err(|e| e.to_string())?;
    isa_builder
        .finish(settings::Flags::new(flags))
        .map_err(|e| e.to_string())
}

thread_local! {
    /// Cranelift's own guidance: reuse one `Context` across compilations so the
    /// per-pass arenas it owns are allocated once instead of per function. We
    /// compile one function at a time per thread, so a thread-local is enough.
    static CTX: std::cell::RefCell<Context> = std::cell::RefCell::new(Context::new());
}

/// Compile one self-contained CLIF function to machine code bytes.
/// Relocation-bearing code (calls, global values) is out of scope for the
/// spike and returns an error instead of silently mis-linking.
pub fn compile_function(func: Function, isa: &dyn TargetIsa) -> Result<Vec<u8>, String> {
    with_ctx(func, isa, |compiled| {
        if !compiled.buffer.relocs().is_empty() {
            return Err("clif compile: relocations not supported in the spike".to_string());
        }
        Ok(compiled.code_buffer().to_vec())
    })
}

/// Compile `func` on the thread's reused `Context` and hand the result to
/// `take` while the Context still owns it. Every Cranelift compilation in the
/// crate goes through here: it is the one place that reuses the arenas and the
/// one place that accounts for backend time.
pub(crate) fn with_ctx<R>(
    func: Function,
    isa: &dyn TargetIsa,
    take: impl FnOnce(&cranelift_codegen::CompiledCode) -> Result<R, String>,
) -> Result<R, String> {
    CTX.with(|cell| {
        // `try_borrow_mut` rather than `borrow_mut`: a re-entrant compile would
        // otherwise panic. Falling back to a fresh Context keeps correctness
        // independent of the optimisation.
        match cell.try_borrow_mut() {
            Ok(mut ctx) => {
                ctx.clear();
                ctx.func = func;
                compile_in(&mut ctx, isa, take)
            }
            Err(_) => compile_in(&mut Context::for_function(func), isa, take),
        }
    })
}

fn compile_in<R>(
    ctx: &mut Context,
    isa: &dyn TargetIsa,
    take: impl FnOnce(&cranelift_codegen::CompiledCode) -> Result<R, String>,
) -> Result<R, String> {
    let start = std::time::Instant::now();
    let res = ctx.compile(isa, &mut ControlPlane::default());
    crate::stats::JIT_STATS
        .backend_time_ns
        .fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
    take(res.map_err(|e| format!("clif compile: {e:?}"))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::JitBuffer;
    use cranelift_codegen::ir::{
        types, AbiParam, InstBuilder, Signature, UserFuncName,
    };
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

    fn exec_buffer(code: &[u8]) -> JitBuffer {
        let mut buf = JitBuffer::new(code.len()).expect("jit buffer");
        buf.as_mut_slice()[..code.len()].copy_from_slice(code);
        buf.make_executable().expect("make_executable");
        buf
    }

    /// (a + b) * c through registers — straight-line codegen + host ABI.
    #[test]
    fn straight_line_arithmetic_executes() {
        let isa = host_isa().expect("host isa");

        let mut sig = Signature::new(isa.default_call_conv());
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));

        let mut func = Function::with_name_signature(UserFuncName::default(), sig);
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut b = FunctionBuilder::new(&mut func, &mut fb_ctx);
        let block = b.create_block();
        b.append_block_params_for_function_params(block);
        b.switch_to_block(block);
        b.seal_block(block);
        let (x, y, z) = {
            let p = b.block_params(block);
            (p[0], p[1], p[2])
        };
        let sum = b.ins().iadd(x, y);
        let prod = b.ins().imul(sum, z);
        b.ins().return_(&[prod]);
        b.finalize();

        let code = compile_function(func, isa.as_ref()).expect("compile");
        let buf = exec_buffer(&code);
        let f: extern "C" fn(i64, i64, i64) -> i64 =
            unsafe { std::mem::transmute(buf.as_ptr()) };
        assert_eq!(f(3, 4, 5), 35);
        assert_eq!(f(-2, 2, 100), 0);
    }

    /// Iterative fib via a loop with block params — CFG, branches and
    /// phi-equivalents through regalloc2, mirroring Varn's SSA shape.
    #[test]
    fn loop_with_block_params_executes() {
        let isa = host_isa().expect("host isa");

        let mut sig = Signature::new(isa.default_call_conv());
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));

        let mut func = Function::with_name_signature(UserFuncName::default(), sig);
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut b = FunctionBuilder::new(&mut func, &mut fb_ctx);

        let entry = b.create_block();
        let header = b.create_block();
        let body = b.create_block();
        let exit = b.create_block();

        b.append_block_params_for_function_params(entry);
        // header(i, a, bb)
        for _ in 0..3 {
            b.append_block_param(header, types::I64);
        }
        b.append_block_param(exit, types::I64);

        b.switch_to_block(entry);
        let n = b.block_params(entry)[0];
        let zero = b.ins().iconst(types::I64, 0);
        let one = b.ins().iconst(types::I64, 1);
        b.ins().jump(header, &[zero.into(), zero.into(), one.into()]);

        b.switch_to_block(header);
        let (i, a, bb) = {
            let p = b.block_params(header);
            (p[0], p[1], p[2])
        };
        let done = b.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThanOrEqual,
            i,
            n,
        );
        b.ins().brif(done, exit, &[a.into()], body, &[]);

        b.switch_to_block(body);
        let next = b.ins().iadd(a, bb);
        let i1 = b.ins().iadd(i, one);
        b.ins().jump(header, &[i1.into(), bb.into(), next.into()]);

        b.switch_to_block(exit);
        let result = b.block_params(exit)[0];
        b.ins().return_(&[result]);

        b.seal_all_blocks();
        b.finalize();

        let code = compile_function(func, isa.as_ref()).expect("compile");
        let buf = exec_buffer(&code);
        let f: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(buf.as_ptr()) };
        assert_eq!(f(0), 0);
        assert_eq!(f(1), 1);
        assert_eq!(f(10), 55);
        assert_eq!(f(35), 9227465);
    }
}
