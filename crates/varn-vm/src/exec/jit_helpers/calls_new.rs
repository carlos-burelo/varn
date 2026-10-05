use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

/// Camino dinámico ÚNICO v2 (§3.2): todo lo no-estático (métodos, closures,
/// `dynamic`) pasa por aquí. Ventana contigua ya preparada por el caller en
/// sus homes; resuelve, ejecuta (entrando a código compilado si existe) y deja
/// boxed en `ctx.jit_native_result` (el retorno directo `-> VmValue` llega con
/// la convención por target, §3.1). Un call-site monomórfico caliente se
/// recompila a estático directo; sin IC inlineado a mano por call-site.
#[varn_op_macros::jit_slow(field = "invoke_dynamic")]
pub(crate) extern "C" fn jit_invoke_dynamic(
    ctx: *mut ExecCtx,
    callee_tag: u64,
    callee_payload: u64,
    act_id: usize,
    arg_start: usize,
    argc: u32,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let callee = VmValue::from_raw_parts(callee_tag, callee_payload);
        let window = ctx_ref.stack.box_range(act_id, arg_start, argc as usize);
        match ctx_ref.invoke(callee, &window) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}
