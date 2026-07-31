//! Allocation-path lowering for CLIF: functions that construct or mutate
//! heap objects. Unlike the alloc-free fast path (fib/matmul), these run a
//! real GC at their loop back-edges, so heap references must survive a
//! moving collection.
//!
//! The mechanism mirrors the template JIT — no stack maps, no native stack
//! walk. Ints stay unboxed in Cranelift Variables; heap-typed registers get
//! a home slot in `ctx.stack[base + reg]`. At each back-edge safepoint the
//! live boxed registers are flushed to their home slots, `gc_safepoint`
//! runs (the collector scans the whole `ctx.stack` as roots and rewrites any
//! promoted/moved index *in place*), then they are reloaded — so the
//! reloaded Variable carries the object's new index. Between safepoints no
//! GC can run (allocation itself never collects), so heap refs live in
//! registers on the common fast path.
//!
//! Soundness rests on two facts verified in the VM: (1) allocation never
//! triggers a collection — only the explicit back-edge safepoint does; and
//! (2) a frame's register window is truncated on return and re-null-filled
//! on entry, so home slots hold only null or valid VmValues (never stale
//! heap-looking garbage the collector could root). See the plan's Fase 5b.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::{Literal, PoolEntry};

use varn_types::register_meta::RegisterMeta;

use varn_types::register_meta::SlotKind;

use super::emit::{
    box_or_pass, call_helper, call_helper_void, meta_is_float, unbox_bool, unbox_f64_coerce,
    use_int, wrap_i48,
};
use super::kinds::K;
use crate::JitHelpers;

/// Whether the function constructs or mutates heap objects — i.e. can run a
/// GC at a back-edge. Such functions take the frame-aware lowering (home
/// slots + safepoints, no readonly resolves). The alloc-free fast path
/// bails on every one of these opcodes, so this is also the exact set that
/// decides which lowering a routed function receives.
pub(super) fn has_alloc(
    code: &[u16],
    pool: &[varn_types::chunk::PoolEntry],
) -> Result<bool, String> {
    let mut ip = 0usize;
    while ip < code.len() {
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        if matches!(
            OpCode::from_u8(code[ip] as u8),
            Some(
                OpCode::BuildArray
                    | OpCode::BuildObject
                    | OpCode::BuildObjectWithShape
                    | OpCode::ArrayPush
                    | OpCode::ArrayExtend
                    | OpCode::MakeEnumVariant
                    | OpCode::StrConcat
                    | OpCode::BuildStr
                    | OpCode::CallNativeOp
                    | OpCode::Add
                    | OpCode::GetProperty
                    | OpCode::SetProperty
                    | OpCode::Call
                    | OpCode::CallMethod
                    | OpCode::InvokeVirtual
                    | OpCode::ToString
                    | OpCode::Typeof
                    | OpCode::Negate
                    | OpCode::GetSymbol
                    | OpCode::StrSlice
                    | OpCode::Intrinsic
                    | OpCode::MakeClosure
                    | OpCode::MakeClass
                    | OpCode::LoadUpvalue
                    | OpCode::StoreUpvalue
                    | OpCode::CloseUpvalue
                    | OpCode::LoadStaticFn
                    | OpCode::LoadModule
                    | OpCode::LoadModuleSlot
                    | OpCode::StoreModuleSlot
                    | OpCode::GetSuper
                    | OpCode::DeclareField
                    | OpCode::Method
                    | OpCode::DefineStatic
                    | OpCode::DefineGetter
                    | OpCode::DefineSetter
                    | OpCode::DefineStaticGetter
                    | OpCode::DefineStaticSetter
                    | OpCode::Inherit
                    | OpCode::BindMethod
                    | OpCode::Try
                    | OpCode::Throw
                    | OpCode::PopTry
                    | OpCode::Yield
                    | OpCode::Await
                    | OpCode::Spawn
                    | OpCode::ObjectRest
                    | OpCode::ObjectKeys
                    | OpCode::ObjectMerge
                    | OpCode::CallSpread
                    | OpCode::WrapSpread
                    | OpCode::LoadGlobal
                    | OpCode::StoreGlobal
                    | OpCode::DefineGlobal
                    | OpCode::GetIndex
                    | OpCode::SetIndex
            )
        ) {
            return Ok(true);
        }
        ip += info.len;
    }
    Ok(false)
}

/// Immutable context threaded through the allocation arms and the safepoint.
pub(super) struct AllocCtx<'a> {
    pub vars: &'a [Variable],
    pub helpers: &'a JitHelpers,
    pub cc: CallConv,
    pub exec_ctx: cranelift_codegen::ir::Value,
    /// The raw `base` parameter: this frame's register-0 index into `ctx.stack`.
    pub base: cranelift_codegen::ir::Value,
    /// The raw `closure` parameter: this function's `VmClosure*`.
    pub closure: cranelift_codegen::ir::Value,
    pub nregs: usize,
    pub register_meta: &'a [RegisterMeta],
}

/// Address of this frame's register-0 home slot, recomputed from `ExecCtx`
/// so a `ctx.stack` reallocation can never leave it stale.
pub(super) fn frame_base_addr(b: &mut FunctionBuilder, actx: &AllocCtx) -> cranelift_codegen::ir::Value {
    let sp = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.stack_data_offset as i32,
    );
    let base_bytes = b.ins().ishl_imm(actx.base, 3);
    b.ins().iadd(sp, base_bytes)
}

/// Load the `this` receiver from register 0's home slot (`stack[base+0]`),
/// where the caller placed it before invoking a method/constructor.
pub(super) fn load_receiver(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
) -> cranelift_codegen::ir::Value {
    load_home(b, actx, 0)
}

pub(super) fn load_home(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    r: usize,
) -> cranelift_codegen::ir::Value {
    let fb = frame_base_addr(b, actx);
    b.ins().load(types::I64, MemFlags::trusted(), fb, (r * 8) as i32)
}

/// Store `reg`'s current value into its `ctx.stack` home slot.
pub(super) fn store_home(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    fb: cranelift_codegen::ir::Value,
    reg: usize,
) {
    let v = box_or_pass(b, actx.vars, state, reg);
    b.ins().store(MemFlags::trusted(), v, fb, (reg * 8) as i32);
}

/// Land a helper's boxed `VmValue` result in `dest`'s Variable. A float-typed
/// register's Variable is `F64`, so raw `I64` result bits must be coerced on
/// the way in — defining it with the bits directly is a Cranelift type error,
/// not a silent miscompile. Mirrors the kind the dataflow assigns to these
/// destinations (`kinds::apply_kinds`, `boxed`/`typed`).
pub(super) fn def_result(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    dest: usize,
    res: cranelift_codegen::ir::Value,
) {
    let fb = frame_base_addr(b, actx);
    if meta_is_float(actx.register_meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else {
        b.def_var(actx.vars[dest], res);
    }
    b.ins().store(MemFlags::trusted(), res, fb, (dest * 8) as i32);
}

/// Materialize a `#[repr(C)]` argument struct for a helper that takes a
/// pointer instead of flat arguments, and return its address. `words` are
/// stored at consecutive 8-byte offsets, matching the struct's field order.
pub(super) fn args_struct(
    b: &mut FunctionBuilder,
    words: &[cranelift_codegen::ir::Value],
) -> cranelift_codegen::ir::Value {
    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        (words.len() * 8) as u32,
        3,
    ));
    for (i, w) in words.iter().enumerate() {
        b.ins().stack_store(*w, slot, (i * 8) as i32);
    }
    b.ins().stack_addr(types::I64, slot, 0)
}

/// Loop back-edge GC safepoint, positioned in the block that ends the loop
/// body. On return the builder sits in a fresh continuation block, ready
/// for the caller to emit the actual back-edge jump. Mirrors the template's
/// `emit_gc_safepoint_check`: inline nursery-fill test, and only the taken
/// branch flushes/collects/reloads.
pub(super) fn emit_backedge_safepoint(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    payload_caches: &[Variable],
) {
    let h = actx.helpers;
    let rcbox = b
        .ins()
        .load(types::I64, MemFlags::trusted(), actx.exec_ctx, h.heap_field_offset as i32);
    let len = b
        .ins()
        .load(types::I64, MemFlags::trusted(), rcbox, h.nursery_len_offset as i32);
    let over = b
        .ins()
        .icmp_imm(IntCC::UnsignedGreaterThanOrEqual, len, h.nursery_threshold as i64);
    let slow = b.create_block();
    let cont = b.create_block();
    b.ins().brif(over, slow, &[], cont, &[]);

    b.switch_to_block(slow);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(b, actx.cc, h.gc_safepoint, &[actx.exec_ctx]);
    reload_boxed(b, actx, state, &regs);
    // A collection just ran, so every resolved payload pointer may dangle:
    // the object can have been evacuated to a different generation and the
    // old-gen `objects` Vec can have moved. Reset the caches to the 0
    // sentinel here, in the TAKEN arm only — the check above is the common
    // path and must stay a test-and-fall-through. This is what lets an
    // allocating function keep a loop payload cache at all; the other half
    // of the argument is that a cached region is allocation-free, so
    // nothing inside it can grow a heap Vec under the pointer.
    let invalid = b.ins().iconst(types::I64, 0);
    for &cv in payload_caches {
        b.def_var(cv, invalid);
    }
    b.ins().jump(cont, &[]);
    b.switch_to_block(cont);
}

/// Registers that could hold a live value at this program point — the
/// set to flush across a helper/GC call.
pub(super) fn live_boxed(actx: &AllocCtx, _state: &[K]) -> Vec<usize> {
    (0..actx.nregs)
        .filter(|&r| !meta_is_float(actx.register_meta, r))
        .collect()
}

/// Spill `regs` to their `ctx.stack` home slots so the collector roots (and
/// rewrites) them. Ints are boxed to home slots and floats stay in F64 vars.
pub(super) fn flush_boxed(b: &mut FunctionBuilder, actx: &AllocCtx, state: &[K], regs: &[usize]) {
    let fb = frame_base_addr(b, actx);
    for &r in regs {
        store_home(b, actx, state, fb, r);
    }
}

/// Reload `regs` from their home slots after a collection may have moved
/// their indices (and grown/moved `ctx.stack`, so recompute the base).
///
/// The restored representation is the EXACT inverse of the one
/// [`store_home`] used — both driven by the flow's kind state, never by
/// `register_meta`. The two authorities disagree in real code (the meta types
/// a slot `Int` while the flow has merged it to a boxed kind at a loop
/// header, and vice versa); restoring by the meta then leaves a raw integer
/// in a register the lowering goes on to read as boxed VmValue bits, which
/// reinterprets a small int as a denormal float — silently wrong comparisons,
/// and an infinite loop when it is a loop counter.
pub(super) fn reload_boxed(b: &mut FunctionBuilder, actx: &AllocCtx, state: &[K], regs: &[usize]) {
    let fb = frame_base_addr(b, actx);
    for &r in regs {
        let v = b
            .ins()
            .load(types::I64, MemFlags::trusted(), fb, (r * 8) as i32);
        if meta_is_float(actx.register_meta, r) {
            let f = unbox_f64_coerce(b, v);
            b.def_var(actx.vars[r], f);
        } else {
            let restored = match state[r] {
                K::Int => wrap_i48(b, v),
                K::Bool => unbox_bool(b, v),
                K::Float => {
                    let f = unbox_f64_coerce(b, v);
                    super::emit::box_f64(b, f)
                }
                _ => v,
            };
            b.def_var(actx.vars[r], restored);
        }
    }
}

/// Cross-function `Call`.
///
/// Two lowerings share one merge point:
///
/// * **direct** — the callee register was proven to hold a specific global,
///   the linker bound that global's closure, and the closure's proto has
///   published a bare (non frame-aware) RAW entry. The call becomes a
///   hardware call into that entry, guarded on the callee's exact `VmValue`
///   bits AND on the entry still being published. A bare raw cannot allocate
///   (`has_alloc` is exactly what makes a lowering frame-aware), so no GC can
///   run under it and nothing needs flushing.
/// * **fallback** — everything else: `clif_call_fallback` runs the callee
///   (interpreter, JIT, constructor, native) and can therefore GC, so live
///   heap refs go to their home slots first and come back after.
///
/// The guard's entry load is what makes the direct path reachable at all:
/// callers compile before their callees, so binding the entry at compile time
/// would resolve to "uncompiled" for essentially every site and never be
/// revisited. See `FunctionProto::clif_raw`.
pub(super) fn emit_call(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    target: Option<&crate::clif::lower::ClifTarget>,
) -> Result<(), String> {
    let w1 = code[ip + 1];
    let w2 = code[ip + 2];
    let dest = (w1 >> 8) as usize;
    let callee_reg = (w1 & 0xFF) as usize;
    // `total` counts the callee/receiver slot at `arg_start`, so the callee's
    // declared parameters are `total - 1` registers starting at `arg_start+1`.
    let total = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    let callee = box_or_pass(b, actx.vars, state, callee_reg);

    let direct = target.filter(|t| {
        t.raw_slot != 0
            && t.param_kinds.len() + 1 == total
            && arg_start + total <= actx.nregs
            // An `int` parameter is handed over unboxed, which `use_int` can
            // only produce from an Int or boxed register — never from a
            // float-typed Variable.
            && t.param_kinds.iter().enumerate().all(|(i, k)| {
                let r = arg_start + 1 + i;
                if *k != SlotKind::Int {
                    return true;
                }
                let unboxable = state[r] == K::Int || super::kinds::is_boxed_kind(state[r]);
                unboxable && !meta_is_float(actx.register_meta, r)
            })
    });

    let Some(t) = direct else {
        let res = emit_vm_call(b, actx, state, callee, arg_start, total);
        def_result(b, actx, dest, res);
        return Ok(());
    };

    // The raw entry unboxes nothing: an `int` parameter arrives as a bare i48
    // payload, every other kind as boxed VmValue bits. Mirrors `build_wrapper`,
    // which stages the same arguments off the VM stack. Materialized before
    // the branch so both arms see values from a dominating block.
    let mut raw_args = Vec::with_capacity(1 + t.param_kinds.len());
    raw_args.push(actx.exec_ctx);
    for (i, k) in t.param_kinds.iter().enumerate() {
        let r = arg_start + 1 + i;
        let v = if *k == SlotKind::Int {
            use_int(b, actx.vars, state, r)?
        } else {
            box_or_pass(b, actx.vars, state, r)
        };
        raw_args.push(v);
    }

    let expected = b.ins().iconst(types::I64, t.expected_bits as i64);
    let same = b.ins().icmp(IntCC::Equal, callee, expected);
    let slot = b.ins().iconst(types::I64, t.raw_slot as i64);
    let raw = b.ins().load(types::I64, MemFlags::trusted(), slot, 0);
    let published = b.ins().icmp_imm(IntCC::NotEqual, raw, 0);
    let take_direct = b.ins().band(same, published);

    let fast = b.create_block();
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I64);
    b.ins().brif(take_direct, fast, &[], slow, &[]);

    b.switch_to_block(fast);
    let raw_sig = {
        let mut s = cranelift_codegen::ir::Signature::new(actx.cc);
        for _ in 0..raw_args.len() {
            s.params
                .push(cranelift_codegen::ir::AbiParam::new(types::I64));
        }
        s.returns
            .push(cranelift_codegen::ir::AbiParam::new(types::I64));
        b.import_signature(s)
    };
    let call = b.ins().call_indirect(raw_sig, raw, &raw_args);
    let raw_res = b.inst_results(call)[0];
    let boxed_fast = super::emit::retag_raw_return(b, raw_res, t.return_kind);
    b.ins().jump(merge, &[boxed_fast.into()]);

    b.switch_to_block(slow);
    let boxed_slow = emit_vm_call(b, actx, state, callee, arg_start, total);
    b.ins().jump(merge, &[boxed_slow.into()]);

    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    def_result(b, actx, dest, res);
    Ok(())
}

/// Dispatch a call through the VM. The helper reads the arguments out of
/// `ctx.stack[base + arg_start ..]`, so the whole call window must be in its
/// home slots first — passing them as C arguments capped the site at three
/// real parameters and silently dropped the rest.
///
/// `flush_boxed` already lands every non-float register; float registers are
/// deliberately outside its set (a float is never a heap reference, so the
/// collector does not need it rooted) but the callee still needs the value,
/// so they are stored here.
fn emit_vm_call(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    callee: cranelift_codegen::ir::Value,
    arg_start: usize,
    total: usize,
) -> cranelift_codegen::ir::Value {
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let fb = frame_base_addr(b, actx);
    for r in arg_start..(arg_start + total).min(actx.nregs) {
        if meta_is_float(actx.register_meta, r) {
            store_home(b, actx, state, fb, r);
        }
    }

    let src = b.ins().iadd_imm(actx.base, arg_start as i64);
    let n = b.ins().iconst(types::I64, total as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.clif_call_fallback,
        &[actx.exec_ctx, callee, src, n],
    );
    reload_boxed(b, actx, state, &regs);
    res
}

/// `BuildArray dest, start, count` — materialize the `count` element
/// `GetProperty first_reg, obj, cs_idx, name_idx` — dynamic/generic property
/// read (`.length`, monomorphic fields, getters). Dispatched through the
/// flat helper, which may run a getter and therefore GC — so live heap refs
/// are flushed/reloaded around it. Result unboxed to int only when the meta
/// proves it. `ip` (next instruction) is handed to the helper for the
/// caller's frame position on the getter path.
pub(super) fn emit_get_property(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    _meta: &[varn_types::register_meta::RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let obj_r = (code[ip + 1] >> 8) as usize;
    let cs_idx = (code[ip + 1] & 0xFF) as usize;
    let name_idx = code[ip + 2] as usize;
    let next_ip = ip + 3;

    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let ci = b.ins().iconst(types::I64, cs_idx as i64);
    let de = b.ins().iconst(types::I64, dest as i64);
    let ipv = b.ins().iconst(types::I64, next_ip as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.get_property_flat,
        &[actx.exec_ctx, actx.closure, actx.base, obj, ni, ci, de, ipv],
    );

    reload_boxed(b, actx, state, &regs);

    def_result(b, actx, dest, res);
}

/// `SetProperty obj(=first_reg), val, cs_idx, name_idx` — dynamic/generic
/// property write (may run a setter → may GC). Same flush/reload discipline
/// as GetProperty; no result.
pub(super) fn emit_set_property(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let obj_r = (code[ip] >> 8) as usize;
    let val_r = (code[ip + 1] >> 8) as usize;
    let cs_idx = (code[ip + 1] & 0xFF) as usize;
    let name_idx = code[ip + 2] as usize;
    let next_ip = ip + 3;

    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let val = box_or_pass(b, actx.vars, state, val_r);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let ci = b.ins().iconst(types::I64, cs_idx as i64);
    let ipv = b.ins().iconst(types::I64, next_ip as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.set_property_flat,
        &[actx.exec_ctx, actx.closure, obj, val, ni, ci, ipv],
    );

    reload_boxed(b, actx, state, &regs);
}

/// `BuildArray dest, start, count` — materialize the `count` element
/// registers into their home slots (the helper reads them from `ctx.stack`)
/// then allocate. Result is a boxed heap reference.
pub(super) fn emit_build_array(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let w1 = code[ip + 1];
    let w2 = code[ip + 2];
    let dest = (w1 >> 8) as usize;
    let start = (w1 & 0xFF) as usize;
    let count = (w2 >> 8) as usize;

    let fb = frame_base_addr(b, actx);
    for i in 0..count {
        store_home(b, actx, state, fb, start + i);
    }
    let start_v = b.ins().iconst(types::I64, start as i64);
    let count_v = b.ins().iconst(types::I64, count as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.build_array,
        &[actx.exec_ctx, actx.base, start_v, count_v],
    );
    def_result(b, actx, dest, res);
}

/// `ArrayPush arr, val` — append a value; the helper carries the old→young
/// write barrier. No result.
pub(super) fn emit_array_push(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let arr_r = (code[ip] >> 8) as usize;
    let val_r = (code[ip + 1] >> 8) as usize;
    let arr = box_or_pass(b, actx.vars, state, arr_r);
    let val = box_or_pass(b, actx.vars, state, val_r);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    call_helper_void(b, actx.cc, actx.helpers.array_push, &[actx.exec_ctx, arr, val]);

    reload_boxed(b, actx, state, &regs);
}

/// `StrConcat dest, a, b` — allocate the concatenation. Result is a boxed
/// heap string.
pub(super) fn emit_str_concat(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let a_r = (code[ip + 1] >> 8) as usize;
    let b_r = (code[ip + 1] & 0xFF) as usize;
    let a = box_or_pass(b, actx.vars, state, a_r);
    let bb = box_or_pass(b, actx.vars, state, b_r);

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let res = call_helper(b, actx.cc, actx.helpers.str_concat, &[actx.exec_ctx, a, bb]);

    reload_boxed(b, actx, state, &regs);

    def_result(b, actx, dest, res);
}

/// `CallNativeOp` — a statically-typed core-type method by op-id (e.g.
/// `arr.push(x)`). Slow-path lowering: materialize receiver+args into their
/// home slots, then `jit_call_native_op(ctx, op_id, base+dest, total)`
/// reads them from `ctx.stack` and returns the result. `dest` = the
/// receiver/call-base register, which also receives the result.
pub(super) fn emit_call_native_op(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    _meta: &[RegisterMeta],
    code: &[u16],
    pool: &[PoolEntry],
    ip: usize,
) -> Result<(), String> {
    let dest = (code[ip] >> 8) as usize;
    let cidx = code[ip + 1] as usize;
    let total = code[ip + 2] as usize;
    let op_id = match pool.get(cidx) {
        Some(PoolEntry::Literal(Literal::Int(i))) => *i as u64,
        _ => return Err("clif: native op-id not an int constant".into()),
    };

    // Flush EVERY live register, not just the argument window: the reload
    // after the call restores all of them from their home slots, so a
    // register left unflushed comes back as whatever `ctx.stack` happened to
    // hold (usually zero) — silently destroying a live value. The extra loop
    // over the argument window covers float-typed args, which `flush_boxed`
    // skips but the native call still needs materialized as boxed values.
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let fb = frame_base_addr(b, actx);
    for r in dest..dest + total {
        if meta_is_float(actx.register_meta, r) {
            store_home(b, actx, state, fb, r);
        }
    }
    let args_start = b.ins().iadd_imm(actx.base, dest as i64);
    let total_v = b.ins().iconst(types::I64, total as i64);
    // The op-id → native fn mapping is fixed for the process, so resolve it
    // HERE and embed the address. The op-id form pays a hash lookup on every
    // single call, which on a hot `arr.push(x)` is a large share of the cost
    // of the call itself. `0` means the table does not know the id — keep the
    // dynamic form so the helper raises the same error it always did.
    let addr = (actx.helpers.resolve_native_op)(op_id);
    let res = if addr != 0 {
        let fn_v = b.ins().iconst(types::I64, addr as i64);
        call_helper(
            b,
            actx.cc,
            actx.helpers.jit_call_native_fnptr,
            &[actx.exec_ctx, fn_v, args_start, total_v],
        )
    } else {
        let op_id_v = b.ins().iconst(types::I64, op_id as i64);
        call_helper(
            b,
            actx.cc,
            actx.helpers.jit_call_native_op,
            &[actx.exec_ctx, op_id_v, args_start, total_v],
        )
    };
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
    Ok(())
}

/// `BuildObjectWithShape dest, start, shape_idx` — allocate an object from a
/// precompiled shape. `dest`/`start` live in `w1`; the helper reads `count`
/// field values from `ctx.stack[base+start..]`, so materialize them first.
/// `count` is resolved from the shape at compile time (in `lower`).
pub(super) fn emit_build_object_with_shape(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    count: usize,
) {
    let w1 = code[ip + 1];
    let dest = (w1 >> 8) as usize;
    let start = (w1 & 0xFF) as usize;
    let shape_idx = code[ip + 2] as usize;

    let fb = frame_base_addr(b, actx);
    for i in 0..count {
        store_home(b, actx, state, fb, start + i);
    }
    let start_v = b.ins().iconst(types::I64, start as i64);
    let shape_v = b.ins().iconst(types::I64, shape_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.build_object_with_shape,
        &[actx.exec_ctx, actx.closure, actx.base, start_v, shape_v],
    );
    def_result(b, actx, dest, res);
}

pub(super) fn emit_make_enum_variant(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    // The helper decodes from the FIRST OPERAND word (the interpreter's
    // post-opcode `ip`), like `BuildObject`/`ObjectRest`.
    let ip_v = b.ins().iconst(types::I64, (ip + 1) as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.make_enum_variant,
        &[actx.exec_ctx, ip_v],
    );

    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_get_enum_tag(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;

    let val = box_or_pass(b, actx.vars, state, src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.get_enum_tag,
        &[actx.exec_ctx, val],
    );

    reload_boxed(b, actx, state, &regs);

    if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}

pub(super) fn emit_build_str(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let count = (code[ip + 1] >> 8) as usize;

    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        (count * 8) as u32,
        3,
    ));
    let parts_ptr = b.ins().stack_addr(types::I64, slot, 0);

    let vals: Vec<_> = (0..count)
        .map(|i| {
            let r = (code[ip + 2 + i] >> 8) as usize;
            box_or_pass(b, actx.vars, state, r)
        })
        .collect();

    for (i, val) in vals.into_iter().enumerate() {
        b.ins().store(MemFlags::trusted(), val, parts_ptr, (i * 8) as i32);
    }

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let count_v = b.ins().iconst(types::I64, count as i64);

    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.build_str,
        &[actx.exec_ctx, parts_ptr, count_v],
    );

    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_intrinsic(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let w1 = code[ip + 1];
    let wire_byte = (w1 >> 8) as usize;
    let arg_count = (w1 & 0xFF) as usize;

    let fb = frame_base_addr(b, actx);
    for r in dest..dest + arg_count {
        store_home(b, actx, state, fb, r);
    }

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let wire_v = b.ins().iconst(types::I64, wire_byte as i64);
    let start_v = b.ins().iadd_imm(actx.base, dest as i64);
    let count_v = b.ins().iconst(types::I64, arg_count as i64);

    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.dispatch_intrinsic,
        &[actx.exec_ctx, wire_v, start_v, count_v],
    );

    reload_boxed(b, actx, state, &regs);

    if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}

pub(super) fn emit_to_string(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;

    let v = box_or_pass(b, actx.vars, state, src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let res = call_helper(b, actx.cc, actx.helpers.to_string, &[actx.exec_ctx, v]);

    reload_boxed(b, actx, state, &regs);

    if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}

pub(super) fn emit_load_upvalue(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let uv_idx = (code[ip + 1] & 0xFF) as usize;
    let uv_v = b.ins().iconst(types::I64, uv_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.load_upvalue,
        &[actx.exec_ctx, actx.closure, uv_v],
    );
    def_result(b, actx, dest, res);
}

pub(super) fn emit_store_upvalue(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let uv_idx = (code[ip + 1] >> 8) as usize;
    let src_r = (code[ip + 1] & 0xFF) as usize;
    let uv_v = b.ins().iconst(types::I64, uv_idx as i64);
    let val = box_or_pass(b, actx.vars, state, src_r);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.store_upvalue,
        &[actx.exec_ctx, actx.closure, uv_v, val],
    );
}

pub(super) fn emit_close_upvalue(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let reg = (code[ip + 1] >> 8) as usize;
    let fb = frame_base_addr(b, actx);
    store_home(b, actx, state, fb, reg);
    let reg_v = b.ins().iconst(types::I64, reg as i64);
    call_helper_void(b, actx.cc, actx.helpers.close_upvalue, &[actx.exec_ctx, reg_v]);
}

pub(super) fn emit_make_closure(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let uv_count = (code[ip + 1] & 0xFF) as usize;
    let fb = frame_base_addr(b, actx);
    for i in 0..uv_count {
        let uv_desc = code[ip + 3 + i];
        let is_local = (uv_desc >> 8) != 0;
        let index = (uv_desc & 0xFF) as usize;
        if is_local {
            store_home(b, actx, state, fb, index);
        }
    }
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let ip_v = b.ins().iconst(types::I64, ip as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.make_closure,
        &[actx.exec_ctx, actx.closure, ip_v, actx.base],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_load_static_fn(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let proto_idx = code[ip + 1] as usize;
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let idx_v = b.ins().iconst(types::I64, proto_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.load_static_fn,
        &[actx.exec_ctx, actx.closure, idx_v],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_load_module(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let spec_idx = code[ip + 1] as usize;
    // The imported module may suspend on a top-level `await`, in which case
    // the helper does not return — it parks this frame at `ip` and longjmps
    // out, and the interpreter re-executes the load. Every register must be
    // in its home slot for that hand-off, exactly as at an `Await`.
    let fb = frame_base_addr(b, actx);
    for r in 0..actx.vars.len() {
        store_home(b, actx, state, fb, r);
    }
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let spec_v = b.ins().iconst(types::I64, spec_idx as i64);
    let own_ip_v = b.ins().iconst(types::I64, ip as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.load_module,
        &[actx.exec_ctx, actx.closure, spec_v, own_ip_v],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_load_module_slot(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src_r = (code[ip + 1] >> 8) as usize;
    let slot_idx = code[ip + 2] as usize;
    let mod_val = box_or_pass(b, actx.vars, state, src_r);
    let slot_v = b.ins().iconst(types::I64, slot_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.load_module_slot,
        &[actx.exec_ctx, mod_val, slot_v],
    );
    def_result(b, actx, dest, res);
}

pub(super) fn emit_store_module_slot(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    // `StoreModuleSlot val_reg, slot_idx` — two words. The destination module
    // is the frame's own export object (the helper looks it up), not an
    // operand.
    let val_r = (code[ip] >> 8) as usize;
    let slot_idx = code[ip + 1] as usize;
    let val = box_or_pass(b, actx.vars, state, val_r);
    let slot_v = b.ins().iconst(types::I64, slot_idx as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.store_module_slot,
        &[actx.exec_ctx, slot_v, val],
    );
}

/// `MakeClass dest, super_reg, name_idx` — allocate the class object and, when
/// a superclass register is named, inherit from it. `super_reg == 0` means "no
/// superclass" (register 0 is the callee/`this` slot, never a class operand),
/// exactly as the interpreter reads it.
pub(super) fn emit_make_class(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let super_reg = (code[ip + 1] >> 8) as usize;
    let name_idx = code[ip + 2] as usize;
    let super_val = if super_reg == 0 {
        b.ins()
            .iconst(types::I64, varn_types::VmValue::null().0 as i64)
    } else {
        box_or_pass(b, actx.vars, state, super_reg)
    };
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let idx_v = b.ins().iconst(types::I64, name_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.make_class,
        &[actx.exec_ctx, actx.closure, super_val, idx_v],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

/// `Method|DefineStatic|DefineGetter|DefineSetter|DefineStaticGetter|
/// DefineStaticSetter class_reg, fn_reg, name_idx` — attach one member to a
/// class. `DeclareField` and `Inherit` share the shape family but have their
/// own helpers (the member helper only knows the six method kinds).
pub(super) fn emit_class_member_op(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    op: OpCode,
    code: &[u16],
    ip: usize,
) -> Result<(), String> {
    let class_reg = (code[ip + 1] >> 8) as usize;
    let class_val = box_or_pass(b, actx.vars, state, class_reg);
    let regs = live_boxed(actx, state);

    if op == OpCode::Inherit {
        // `Inherit class_reg, super_reg` — no name constant.
        let super_val = box_or_pass(b, actx.vars, state, (code[ip + 1] & 0xFF) as usize);
        flush_boxed(b, actx, state, &regs);
        call_helper_void(
            b,
            actx.cc,
            actx.helpers.inherit,
            &[actx.exec_ctx, class_val, super_val],
        );
        reload_boxed(b, actx, state, &regs);
        return Ok(());
    }

    let name_idx = code[ip + 2] as usize;
    let idx_v = b.ins().iconst(types::I64, name_idx as i64);

    if op == OpCode::DeclareField {
        flush_boxed(b, actx, state, &regs);
        call_helper_void(
            b,
            actx.cc,
            actx.helpers.declare_field,
            &[actx.exec_ctx, actx.closure, class_val, idx_v],
        );
        reload_boxed(b, actx, state, &regs);
        return Ok(());
    }

    // `JitClassMemberArgs { class_val, fn_val, name_idx, kind }` — the kind
    // discriminant the helper switches on.
    let kind = match op {
        OpCode::Method => 0,
        OpCode::DefineStatic => 1,
        OpCode::DefineGetter => 2,
        OpCode::DefineSetter => 3,
        OpCode::DefineStaticGetter => 4,
        OpCode::DefineStaticSetter => 5,
        _ => return Err(format!("clif: not a class member op ({op:?})")),
    };
    let fn_val = box_or_pass(b, actx.vars, state, (code[ip + 1] & 0xFF) as usize);
    let kind_v = b.ins().iconst(types::I64, kind);
    let args = args_struct(b, &[class_val, fn_val, idx_v, kind_v]);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.class_member_op,
        &[actx.exec_ctx, actx.closure, args],
    );
    reload_boxed(b, actx, state, &regs);
    Ok(())
}

pub(super) fn emit_get_super(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let name_idx = code[ip + 1] as usize;
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let idx_v = b.ins().iconst(types::I64, name_idx as i64);
    let res = call_helper(b, actx.cc, actx.helpers.get_super, &[actx.exec_ctx, idx_v]);
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_load_global(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let name_idx = code[ip + 1] as usize;
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let idx_v = b.ins().iconst(types::I64, name_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.load_global,
        &[actx.exec_ctx, actx.closure, idx_v],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

/// `StoreGlobal|DefineGlobal src_reg, name_idx` — the source register lives in
/// the FIRST operand word and the name constant in the second (a three-word
/// instruction), like the interpreter's `exec_variable_op`.
pub(super) fn emit_global_write(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    helper: usize,
    code: &[u16],
    ip: usize,
) {
    let src = (code[ip + 1] >> 8) as usize;
    let name_idx = code[ip + 2] as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let idx_v = b.ins().iconst(types::I64, name_idx as i64);
    call_helper_void(b, actx.cc, helper, &[actx.exec_ctx, val, idx_v]);
    reload_boxed(b, actx, state, &regs);
}

pub(super) fn emit_get_index(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let obj_r = (code[ip + 1] >> 8) as usize;
    let idx_r = (code[ip + 1] & 0xFF) as usize;
    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let idx = box_or_pass(b, actx.vars, state, idx_r);
    // `JitGetIndexArgs { obj, key, dest }`.
    let dest_v = b.ins().iconst(types::I64, dest as i64);
    let args = args_struct(b, &[obj, idx, dest_v]);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(b, actx.cc, actx.helpers.get_index, &[actx.exec_ctx, args]);
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_set_index(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let obj_r = (code[ip] >> 8) as usize;
    let idx_r = (code[ip + 1] >> 8) as usize;
    let val_r = (code[ip + 1] & 0xFF) as usize;
    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let idx = box_or_pass(b, actx.vars, state, idx_r);
    let val = box_or_pass(b, actx.vars, state, val_r);
    // `JitSetIndexArgs { obj, key, val }`.
    let args = args_struct(b, &[obj, idx, val]);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(b, actx.cc, actx.helpers.set_index, &[actx.exec_ctx, args]);
    reload_boxed(b, actx, state, &regs);
}

pub(super) fn emit_try_push(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    code: &[u16],
    ip: usize,
) {
    let err_reg = (code[ip + 1] >> 8) as u32;
    let offset_hi = code[ip + 2] as usize;
    let offset_lo = code[ip + 3] as usize;
    let catch_offset = (offset_hi << 16) | offset_lo;
    let catch_ip = ip + 4 + catch_offset;
    let ip_v = b.ins().iconst(types::I64, catch_ip as i64);
    let err_v = b.ins().iconst(types::I64, err_reg as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.try_push,
        &[actx.exec_ctx, ip_v, err_v],
    );
}

pub(super) fn emit_try_pop(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
) {
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.try_pop,
        &[actx.exec_ctx],
    );
}

pub(super) fn emit_throw(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.throw,
        &[actx.exec_ctx, val],
    );
}

pub(super) fn emit_yield(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest_reg = (code[ip + 1] >> 8) as u32;
    let src = (code[ip + 1] & 0xFF) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    let fb = frame_base_addr(b, actx);
    for r in 0..actx.vars.len() {
        store_home(b, actx, state, fb, r);
    }
    let dest_v = b.ins().iconst(types::I64, dest_reg as i64);
    let resume_v = b.ins().iconst(types::I64, (ip + 2) as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.yield_helper,
        &[actx.exec_ctx, val, dest_v, resume_v],
    );
}

pub(super) fn emit_await(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    let fb = frame_base_addr(b, actx);
    for r in 0..actx.vars.len() {
        store_home(b, actx, state, fb, r);
    }
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let dest_v = b.ins().iconst(types::I64, dest as i64);
    let resume_v = b.ins().iconst(types::I64, (ip + 2) as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.await_helper,
        &[actx.exec_ctx, val, dest_v, resume_v],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_spawn(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.spawn,
        &[actx.exec_ctx, val],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_build_object(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let count = (code[ip + 1] & 0xFF) as usize;
    let fb = frame_base_addr(b, actx);
    let mut cur_ip = ip + 2;
    for _ in 0..count {
        cur_ip += 1; // skip k_idx
        let val_reg = (code[cur_ip] >> 8) as usize;
        cur_ip += 1;
        store_home(b, actx, state, fb, val_reg);
    }
    // The helper re-walks the instruction from its FIRST OPERAND word (the
    // interpreter's post-opcode `ip`), not from the opcode word.
    let ipv = b.ins().iconst(types::I64, (ip + 1) as i64);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.build_object,
        &[actx.exec_ctx, actx.closure, actx.base, ipv],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_object_rest(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let src = (code[ip + 1] & 0xFF) as usize;
    let fb = frame_base_addr(b, actx);
    store_home(b, actx, state, fb, src);
    // Same convention as `BuildObject`: the helper decodes from the first
    // operand word.
    let ipv = b.ins().iconst(types::I64, (ip + 1) as i64);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.object_rest,
        &[actx.exec_ctx, ipv],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_object_keys(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let obj = box_or_pass(b, actx.vars, state, src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.object_keys,
        &[actx.exec_ctx, obj],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_object_merge(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest_reg = (code[ip] >> 8) as usize;
    let src_reg = (code[ip + 1] >> 8) as usize;
    let dest_val = box_or_pass(b, actx.vars, state, dest_reg);
    let src_val = box_or_pass(b, actx.vars, state, src_reg);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.object_merge,
        &[actx.exec_ctx, dest_val, src_val],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest_reg, res);
}

pub(super) fn emit_get_property_maybe(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let obj_r = (code[ip + 1] >> 8) as usize;
    let name_idx = code[ip + 2] as usize;
    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.get_property_maybe,
        &[actx.exec_ctx, obj, ni],
    );
    def_result(b, actx, dest, res);
}

pub(super) fn emit_bind_method(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    // `BindMethod dest, obj, name_idx` — dest and receiver BOTH live in the
    // first operand word (hi/lo), unlike the property ops above.
    let dest = (code[ip + 1] >> 8) as usize;
    let obj_r = (code[ip + 1] & 0xFF) as usize;
    let name_idx = code[ip + 2] as usize;
    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.bind_method,
        &[actx.exec_ctx, obj, ni],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_assert_not_null(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.assert_not_null,
        &[actx.exec_ctx, val],
    );
}

pub(super) fn emit_array_extend(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let arr_r = (code[ip] >> 8) as usize;
    let src_r = (code[ip + 1] >> 8) as usize;
    let arr = box_or_pass(b, actx.vars, state, arr_r);
    let src = box_or_pass(b, actx.vars, state, src_r);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.array_extend,
        &[actx.exec_ctx, arr, src],
    );
    reload_boxed(b, actx, state, &regs);
}

pub(super) fn emit_wrap_spread(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.wrap_spread,
        &[actx.exec_ctx, val],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(super) fn emit_call_spread(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[varn_types::register_meta::RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let w1 = code[ip + 1];
    let w2 = code[ip + 2];
    let dest = (w1 >> 8) as usize;
    let callee_reg = (w1 & 0xFF) as usize;
    let argc = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    let callee = box_or_pass(b, actx.vars, state, callee_reg);
    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        40,
        0,
    ));

    let arg_start_v = b.ins().iconst(types::I64, arg_start as i64);
    let argc_v = b.ins().iconst(types::I64, argc as i64);
    let dest_v = b.ins().iconst(types::I64, dest as i64);
    let next_ip_v = b.ins().iconst(types::I64, (ip + 3) as i64);

    b.ins().stack_store(callee, slot, 0);
    b.ins().stack_store(arg_start_v, slot, 8);
    b.ins().stack_store(argc_v, slot, 16);
    b.ins().stack_store(dest_v, slot, 24);
    b.ins().stack_store(next_ip_v, slot, 32);

    let slot_addr = b.ins().stack_addr(types::I64, slot, 0);

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.call_spread,
        &[actx.exec_ctx, slot_addr],
    );

    reload_boxed(b, actx, state, &regs);

    if meta.get(dest).map_or(false, |m| m.kind == varn_types::register_meta::SlotKind::Int) {
        let s = b.ins().ishl_imm(res, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(actx.vars[dest], un);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}

pub(super) fn emit_invoke_runtime_static(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    _state: &[K],
    code: &[u16],
    ip: usize,
) -> Result<(), String> {
    let dest = (code[ip + 1] >> 8) as usize;
    let start_reg = (code[ip + 3] & 0xFF) as usize;
    let end_reg = (code[ip + 4] >> 8) as usize;
    let flag = (code[ip + 4] & 0xFF) as usize;

    let start_v = b.ins().iconst(types::I64, start_reg as i64);
    let end_v = b.ins().iconst(types::I64, end_reg as i64);
    let flag_v = b.ins().iconst(types::I64, flag as i64);

    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.range,
        &[actx.exec_ctx, start_v, end_v, flag_v],
    );
    def_result(b, actx, dest, res);
    Ok(())
}


