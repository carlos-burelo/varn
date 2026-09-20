//! Home-slot access for compiled frames, against the partitioned
//! [`crate::frame_store::FrameStore`].
//!
//! A compiled frame keeps its live values in native registers (`Variable`s);
//! the HOME slot is the frame's memory copy, read back by the GC, by OSR, and
//! by an interpreted resume. Phase B routes that memory traffic through these
//! two helpers instead of inlining `stack[base + reg]` arithmetic: the class
//! of each register (`Gpr`/`Fpr`/`Ref`/`Dyn`) and the activation's per-class
//! bases live in `FrameStore`, which generated code cannot cheaply reproduce.
//! One mechanism, one owner (Ley 3/8); inlining is a later optimization.
//!
//! `act_id` is the `FrameStore` activation id (the JIT's `base` parameter),
//! NOT a stack offset. `reg` is a VM register number; the helper resolves its
//! class and index through `FrameStore::addr_of`.

use crate::exec::ctx::ExecCtx;
use crate::frame_store::REF_UNINIT;
use crate::value::VmValue;

/// Write a boxed `VmValue` (tag + payload) into register `reg`'s home slot of
/// activation `act_id`, converting to the slot's physical class.
///
/// The class is checker-proven, so a conversion mismatch is a compiler bug,
/// not user input: it is asserted in debug and ignored in release (the type
/// proof is what makes silently storing nothing sound — a value the checker
/// typed for this slot can only be an `int`/`float`/ref/bits of the right
/// shape).
pub(crate) extern "C" fn jit_store_home(
    ctx: *mut ExecCtx,
    act_id: usize,
    reg: usize,
    tag: u64,
    payload: u64,
) {
    // SAFETY: generated code passes the live `ExecCtx` and its own activation.
    let ctx = unsafe { &mut *ctx };
    let addr = ctx.stack.addr_of(act_id, reg);
    let v = VmValue::from_raw_parts(tag, payload);
    if ctx.stack.set_addr(addr, v).is_err() {
        debug_assert!(false, "jit_store_home: class mismatch for register {reg}");
    }
}

/// Read register `reg`'s home slot of activation `act_id` as a boxed
/// `VmValue`, writing it through `out`. A never-written `Ref` home reads
/// `null` (via `REF_UNINIT`), symmetric with `FrameStore::get_addr`.
pub(crate) extern "C" fn jit_load_home(
    ctx: *mut ExecCtx,
    act_id: usize,
    reg: usize,
    out: *mut VmValue,
) {
    // SAFETY: `ctx` is live and `out` points at a stack slot the caller owns.
    let ctx = unsafe { &*ctx };
    let addr = ctx.stack.addr_of(act_id, reg);
    let v = ctx.stack.get_addr(addr);
    debug_assert!(addr.idx != REF_UNINIT || v.is_null());
    unsafe { *out = v };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::ctx::ExecCtx;
    use std::rc::Rc;
    use varn_types::register_meta::{RegisterMeta, SlotKind};
    use varn_types::FunctionProto;

    fn ctx_with_frame(kinds: &[SlotKind]) -> (ExecCtx, usize) {
        let mut proto = FunctionProto::default();
        proto.register_count = kinds.len() as u16;
        proto.register_meta = kinds.iter().map(|&kind| RegisterMeta { kind }).collect();
        let proto = Rc::new(proto);
        let mut ctx = ExecCtx::new(
            crate::globals::GlobalStore::default(),
            crate::settings::ExecSettings::default(),
        );
        let id = ctx.stack.push_frame(&proto);
        (ctx, id)
    }

    #[test]
    fn store_and_load_roundtrip_per_class() {
        // r0 = Int (Gpr), r1 = Float (Fpr), r2 = Dynamic (Dyn).
        let (mut ctx, id) = ctx_with_frame(&[SlotKind::Int, SlotKind::Float, SlotKind::Dynamic]);

        jit_store_home(&mut ctx as *mut _, id, 0, varn_types::vm_value::KIND_INT, 42);
        assert_eq!(ctx.stack.g(id, 0), 42);

        jit_store_home(
            &mut ctx as *mut _,
            id,
            1,
            varn_types::vm_value::KIND_FLOAT,
            2.5f64.to_bits(),
        );
        assert_eq!(ctx.stack.f(id, 1), 2.5);

        jit_store_home(
            &mut ctx as *mut _,
            id,
            2,
            varn_types::vm_value::KIND_BOOL,
            1,
        );
        let mut out = VmValue::null();
        jit_load_home(&mut ctx as *mut _, id, 2, &mut out as *mut _);
        assert!(out.is_bool());

        let mut gi = VmValue::null();
        jit_load_home(&mut ctx as *mut _, id, 0, &mut gi as *mut _);
        assert_eq!(gi.as_int(), 42);
    }

    #[test]
    fn uninit_ref_home_reads_null() {
        let (mut ctx, id) = ctx_with_frame(&[SlotKind::Ref]);
        let mut out = VmValue::from_int(7);
        jit_load_home(&mut ctx as *mut _, id, 0, &mut out as *mut _);
        assert!(out.is_null());
        assert!(ctx.stack.r(id, 0) == varn_types::register_meta::REF_UNINIT);
    }
}
