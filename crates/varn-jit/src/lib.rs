pub mod aot;
pub mod clif;
pub(crate) mod loop_hoist;
pub mod mem;
pub mod stack_roots;
pub mod stats;

pub use stats::{CompileOutcome, CompileRecord, JitStats, JitStatsSnapshot, JIT_STATS};

pub use loop_hoist::{
    diagnose_loops, is_alloc_free_op, CacheSource, HoistCandidate, LoopDiagnostic,
};

pub use varn_abi;

pub use cranelift_codegen::isa::OwnedTargetIsa;

use std::any::Any;
use std::rc::Rc;
use varn_op_macros::jit_helper_table;
use varn_types::FunctionProto;
use varn_types::VmValue;

pub type JitFn = unsafe extern "C" fn(
    ctx: *mut std::ffi::c_void,
    closure: *const std::ffi::c_void,
    base: usize,
    exec_ctx: *mut std::ffi::c_void,
) -> VmValue;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitArrayLayout {
    pub state_off: usize,

    pub kind_off: usize,

    pub young_state: usize,

    pub vec_ptr_off: usize,

    pub array_tag: usize,

    pub str_tag: usize,

    pub payload_off: usize,

    pub disc_off: usize,

    pub elems_ptr_off: usize,
    pub elems_len_off: usize,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct JitObjectLayout {
    pub object_tag: usize,

    pub instance_tag: usize,

    pub payload_off: usize,

    pub instance_data_off: usize,

    pub len_off: usize,

    pub values_off: usize,

    pub instance_values_off: usize,

    pub instance_class_id_off: usize,

    pub shape_off: usize,

    pub shape_id_off: usize,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct JitCallLayout {
    pub closure_tag: usize,

    pub closure_payload_off: usize,

    pub rc_value_off: usize,

    pub rc_strong_off: usize,

    pub closure_proto_off: usize,

    pub proto_native_off: usize,
    pub proto_native_sig_off: usize,
    pub proto_epoch_off: usize,

    pub frames_ptr_off: usize,
    pub frames_len_off: usize,
    pub frames_cap_off: usize,

    pub frame_size: usize,
    pub frame_closure_ptr_off: usize,
    pub frame_owned_off: usize,
    pub frame_ip_off: usize,
    pub frame_base_off: usize,
    pub frame_class_off: usize,
    pub frame_return_reg_off: usize,

    pub class_vtable_ptr_off: usize,
    pub class_vtable_len_off: usize,
    pub class_vtable_version_off: usize,
    pub no_activation: usize,
    pub no_return_reg: usize,
    pub max_call_depth: usize,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct JitFrameLayout {
    pub gpr_ptr_offset: usize,
    pub fpr_ptr_offset: usize,
    pub refs_ptr_offset: usize,
    pub dyn_ptr_offset: usize,

    pub allocs_ptr_offset: usize,

    pub alloc_size: usize,

    pub alloc_bases_offset: usize,
}

macro_rules! define_tail {
    ( $( $field:ident ),* $(,)? ) => {
        #[derive(Debug, Clone, Copy)]
        #[repr(C)]
        pub struct JitHelpers {
            $( pub $field: usize, )*











    pub resolve_native_op: fn(u64) -> varn_types::NativeOpTarget,

        pub array_layout: JitArrayLayout,

        pub object_layout: JitObjectLayout,

        pub heap_field_offset: usize,

        pub young_len_offset: usize,

        pub young_threshold: usize,
        pub jit_native_result_offset: usize,
        pub jit_exit_offset: usize,




        pub globals_offset: usize,


        pub globals_store_offset: usize,




        pub closure_module_base_offset: usize,



        pub closure_ic_entries_offset: usize,

        pub poly_ic_slot_size: usize,
        pub frame_layout: JitFrameLayout,
        pub call_layout: JitCallLayout,
        }
    };
}

jit_helper_table! { define, "../varn-vm/src/exec/jit_helpers" }

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitGetIndexArgs {
    pub obj: VmValue,
    pub key: VmValue,
    pub dest: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitSetIndexArgs {
    pub obj: VmValue,
    pub key: VmValue,
    pub val: VmValue,
}

use std::sync::atomic::Ordering;

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

pub fn compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: JitHelpers,
    linker: &dyn clif::lower::ClifLinker,
    osr_ip: Option<usize>,
) -> Result<Compiled, String> {
    if clif::enabled() {
        if let Ok(isa) = clif::shared_isa() {
            let start = std::time::Instant::now();
            let res =
                clif::lower::try_compile(proto, constants, &helpers, isa, linker, osr_ip, None);
            let elapsed = start.elapsed().as_nanos() as u64;
            let words = proto.chunk.code.len();
            match res {
                Ok(art) => {
                    if clif::trace() {
                        eprintln!("CLIF ROUTE {:?}", proto.name);
                    }
                    JIT_STATS.compile_success.fetch_add(1, Ordering::Relaxed);
                    JIT_STATS
                        .total_compile_time_ns
                        .fetch_add(elapsed, Ordering::Relaxed);
                    JIT_STATS
                        .total_code_size_bytes
                        .fetch_add(art.buffer.size() as u64, Ordering::Relaxed);
                    record(|| CompileRecord {
                        name: fn_name(proto),
                        words,
                        outcome: CompileOutcome::Routed,
                        compile_ns: elapsed,
                    });
                    let jit_fn: JitFn = unsafe { std::mem::transmute(art.entry) };
                    let (native, native_sig) = art.native_entry(proto);
                    return Ok(Compiled {
                        entry: jit_fn,
                        native,
                        native_sig,
                        code: Rc::new(art) as Rc<dyn Any>,
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
                    return Err(e);
                }
            }
        }
    }

    Err("JIT disabled or unsupported proto".into())
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitSetPropertyArgs {
    pub obj: VmValue,
    pub val: VmValue,
    pub name_idx: usize,
    pub cs_idx: usize,
    pub ip: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitInvokeVirtualArgs {
    pub this_val: VmValue,
    pub name_idx: usize,
    pub arg_start: usize,
    pub arg_count: usize,
    pub dest: usize,
    pub ip: usize,
}
