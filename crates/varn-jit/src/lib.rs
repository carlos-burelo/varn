#[cfg(feature = "clif")]
pub mod aot;
#[cfg(feature = "clif")]
pub mod clif;
#[cfg(not(feature = "clif"))]
pub mod clif {
    pub fn enabled() -> bool {
        false
    }

    pub fn trace() -> bool {
        false
    }

    pub fn shared_isa() -> Result<&'static crate::OwnedTargetIsa, String> {
        Err("clif disabled (build without the `clif` feature)".to_owned())
    }

    pub fn host_isa() -> Result<crate::OwnedTargetIsa, String> {
        Err("clif disabled (build without the `clif` feature)".to_owned())
    }

    pub mod lower {
        use varn_types::FunctionProto;

        pub trait ClifLinker {
            fn current_epoch(&self) -> u64;
        }

        pub fn gate_reason(proto: &FunctionProto) -> Option<String> {
            let words = proto.chunk.code.len();
            if words > crate::SIZE_GATE_WORDS {
                return Some(format!("too large (>{} words)", crate::SIZE_GATE_WORDS));
            }
            None
        }
    }

    pub mod debug {
        use varn_types::{FunctionProto, VmValue};

        use crate::JitHelpers;

        use super::lower::ClifLinker;

        #[derive(Debug, Default, Clone)]
        pub struct KindReport {
            pub nregs: usize,
            pub blocks: Vec<(usize, Vec<String>)>,
        }

        #[derive(Debug, Default, Clone)]
        pub struct CodeBytes {
            pub bytes: Vec<u8>,
            pub raw_off: usize,
            pub raw_len: usize,
            pub entry_off: usize,
        }

        #[derive(Debug, Default)]
        pub struct ClifDebugSink {
            pub kinds: Option<KindReport>,
            pub clif_ir: Option<String>,
            pub code: Option<CodeBytes>,
        }

        pub struct ClifInspection {
            pub name: String,
            pub route: Result<(), String>,
            pub kinds: Option<KindReport>,
            pub clif_ir: Option<String>,
            pub code: Option<CodeBytes>,
            pub frame_aware: bool,
            pub framed: bool,
            pub fa_reasons: Vec<&'static str>,
        }

        pub fn inspect(
            proto: &FunctionProto,
            _constants: &[VmValue],
            _helpers: &JitHelpers,
            _isa: &crate::OwnedTargetIsa,
        ) -> ClifInspection {
            ClifInspection {
                name: proto.name.as_deref().unwrap_or("<top-level>").to_string(),
                route: Err("clif disabled (build without the `clif` feature)".to_owned()),
                kinds: None,
                clif_ir: None,
                code: None,
                frame_aware: false,
                framed: false,
                fa_reasons: Vec::new(),
            }
        }
    }
}
#[cfg(not(feature = "clif"))]
pub mod aot {
    use crate::OwnedTargetIsa;

    pub struct AotOutput {
        pub object_bytes: Vec<u8>,
    }

    pub fn compile_to_object(
        _proto: &varn_types::FunctionProto,
        _isa: &OwnedTargetIsa,
    ) -> Result<AotOutput, String> {
        Err("aot disabled (build without the `clif` feature)".to_owned())
    }
}
pub(crate) mod loop_hoist;
pub mod mem;
pub mod stack_roots;
pub mod stats;

pub use stats::{CompileOutcome, CompileRecord, JitStats, JitStatsSnapshot, JIT_STATS};

pub use loop_hoist::{
    diagnose_loops, is_alloc_free_op, CacheSource, HoistCandidate, LoopDiagnostic,
};

#[cfg(feature = "clif")]
pub use cranelift_codegen::isa::OwnedTargetIsa;

#[cfg(not(feature = "clif"))]
#[derive(Debug)]
pub struct OwnedTargetIsa {
    _private: (),
}

use std::any::Any;
use std::rc::Rc;
pub use varn_jit_abi::{
    JitArrayLayout, JitBufferLayout, JitCallLayout, JitFrameLayout, JitGetIndexArgs, JitHelpers,
    JitInstanceAlloc, JitInvokeVirtualArgs, JitObjectLayout, JitSetIndexArgs, JitSetPropertyArgs,
    JIT_HELPERS_VERSION,
};
use varn_types::FunctionProto;
use varn_types::VmValue;

pub type JitFn = unsafe extern "C" fn(
    ctx: *mut std::ffi::c_void,
    closure: *const std::ffi::c_void,
    base: usize,
    exec_ctx: *mut std::ffi::c_void,
) -> VmValue;

#[cfg(feature = "clif")]
use std::sync::atomic::Ordering;

#[cfg(feature = "clif")]
use stats::record;

pub const SIZE_GATE_WORDS: usize = 8192;

fn fn_name(proto: &FunctionProto) -> String {
    proto.name.as_deref().unwrap_or("<module>").to_owned()
}

pub struct Compiled {
    pub entry: JitFn,
    pub native: usize,
    pub native_sig: u64,
    pub code: Rc<dyn Any>,
}

pub(crate) const PAIR_MIGRATION_PENDING: bool = false;

pub(crate) const PAIR_MIGRATION_BAIL: &str =
    "clif: disabled while `VmValue` moves to a (tag, payload) pair; the lowering still models every register as one machine word";

#[cfg(feature = "clif")]
pub use clif::lower::CodeImage;

#[cfg(not(feature = "clif"))]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct CodeImage;

#[cfg(not(feature = "clif"))]
impl CodeImage {
    pub fn persistable(&self) -> bool {
        false
    }
}

pub fn compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: JitHelpers,
    linker: &dyn clif::lower::ClifLinker,
    osr_req: Option<varn_types::ssa::OsrRequest>,
) -> Result<Compiled, String> {
    let image = lower(proto, constants, helpers, osr_req)?;
    install(&image, proto, constants, helpers, linker)
}

#[allow(unused_variables)]
pub fn lower(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: JitHelpers,
    osr_req: Option<varn_types::ssa::OsrRequest>,
) -> Result<CodeImage, String> {
    #[cfg(feature = "clif")]
    if clif::enabled()
        && let Ok(isa) = clif::shared_isa()
    {
        let start = std::time::Instant::now();
        let res = clif::lower::try_compile(proto, constants, &helpers, isa, osr_req, None);
        let elapsed = start.elapsed().as_nanos() as u64;
        let words = proto.chunk.code.len();
        match &res {
            Ok(image) => {
                if clif::trace() {
                    eprintln!("CLIF ROUTE {:?}", proto.name);
                }
                JIT_STATS.compile_success.fetch_add(1, Ordering::Relaxed);
                JIT_STATS
                    .total_compile_time_ns
                    .fetch_add(elapsed, Ordering::Relaxed);
                JIT_STATS
                    .total_code_size_bytes
                    .fetch_add(image.code_len() as u64, Ordering::Relaxed);
                record(|| CompileRecord {
                    name: fn_name(proto),
                    words,
                    outcome: CompileOutcome::Routed,
                    compile_ns: elapsed,
                });
            }
            Err(e) => {
                if clif::trace() {
                    eprintln!("CLIF BAIL  {:?}: {e}", proto.name);
                }
                JIT_STATS.compile_fail.fetch_add(1, Ordering::Relaxed);
                JIT_STATS
                    .total_compile_time_ns
                    .fetch_add(elapsed, Ordering::Relaxed);
                record(|| CompileRecord {
                    name: fn_name(proto),
                    words,
                    outcome: CompileOutcome::Bailed(e.clone()),
                    compile_ns: elapsed,
                });
            }
        }
        return res;
    }

    Err("JIT disabled or unsupported proto".into())
}

#[allow(unused_variables)]
pub fn install(
    image: &CodeImage,
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: JitHelpers,
    linker: &dyn clif::lower::ClifLinker,
) -> Result<Compiled, String> {
    #[cfg(feature = "clif")]
    {
        let resolver = clif::reloc::Resolver {
            helpers: &helpers,
            proto,
            constants,
            natives: &[],
            epoch: linker.current_epoch(),
        };
        let art = image.install(&resolver)?;
        let jit_fn: JitFn = unsafe { std::mem::transmute(art.entry) };
        let (native, native_sig) = art.native_entry(proto);
        Ok(Compiled {
            entry: jit_fn,
            native,
            native_sig,
            code: Rc::new(art) as Rc<dyn Any>,
        })
    }
    #[cfg(not(feature = "clif"))]
    Err("JIT disabled or unsupported proto".into())
}
