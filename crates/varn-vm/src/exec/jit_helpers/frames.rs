use crate::exec::ctx::ExecCtx;
use crate::exec::jit_helpers::construct::jit_propagate_error;

/// `LoadStaticFn` out of compiled code: the closure of the running closure's
/// function constant `proto_idx`, which captures nothing.
#[varn_op_macros::jit_slow(field = "load_static_fn")]
pub(crate) extern "C" fn jit_load_static_fn(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    proto_idx: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.make_closure(&*closure, proto_idx, 0, std::iter::empty()) {
            Ok(val) => ctx_ref.jit_native_result = val,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}
