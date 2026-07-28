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

use varn_types::register_meta::{RegisterMeta, SlotKind};

use super::emit::{box_or_pass, call_helper, call_helper_void, meta_is_float, unbox_f64_coerce};
use super::kinds::{is_boxed_kind, K};
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
                    // A native core-type method may allocate (push, concat,
                    // map…), so conservatively force the safepoint discipline.
                    | OpCode::CallNativeOp
                    // Generic `Add` handles string concatenation, which
                    // allocates a heap string.
                    | OpCode::Add
                    // A property read/write may invoke a getter/setter
                    // (arbitrary VM code that can allocate); needs closure.
                    | OpCode::GetProperty
                    | OpCode::SetProperty
                    | OpCode::CallMethod
                    | OpCode::InvokeVirtual
                    // These allocate a heap string / bind a method / make a
                    // BigInt — force the safepoint discipline in loops.
                    | OpCode::ToString
                    | OpCode::Typeof
                    | OpCode::Negate
                    | OpCode::GetSymbol
                    // A string slice allocates a Slice HeapStr (and it's not
                    // an int-contract callee, so no fast-IC regression).
                    | OpCode::StrSlice
                    | OpCode::Intrinsic
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
    let fb = frame_base_addr(b, actx);
    b.ins().load(types::I64, MemFlags::trusted(), fb, 0)
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

/// Loop back-edge GC safepoint, positioned in the block that ends the loop
/// body. On return the builder sits in a fresh continuation block, ready
/// for the caller to emit the actual back-edge jump. Mirrors the template's
/// `emit_gc_safepoint_check`: inline nursery-fill test, and only the taken
/// branch flushes/collects/reloads.
pub(super) fn emit_backedge_safepoint(b: &mut FunctionBuilder, actx: &AllocCtx, state: &[K]) {
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
    reload_boxed(b, actx, &regs);
    b.ins().jump(cont, &[]);
    b.switch_to_block(cont);
}

/// Registers that could hold a heap reference at this program point — the
/// set to root across a collection.
pub(super) fn live_boxed(actx: &AllocCtx, state: &[K]) -> Vec<usize> {
    (0..actx.nregs).filter(|&r| is_boxed_kind(state[r])).collect()
}

/// Spill `regs` to their `ctx.stack` home slots so the collector roots (and
/// rewrites) them. Ints are GC-irrelevant and stay in registers.
pub(super) fn flush_boxed(b: &mut FunctionBuilder, actx: &AllocCtx, state: &[K], regs: &[usize]) {
    let fb = frame_base_addr(b, actx);
    for &r in regs {
        store_home(b, actx, state, fb, r);
    }
}

/// Reload `regs` from their home slots after a collection may have moved
/// their indices (and grown/moved `ctx.stack`, so recompute the base).
pub(super) fn reload_boxed(b: &mut FunctionBuilder, actx: &AllocCtx, regs: &[usize]) {
    let fb = frame_base_addr(b, actx);
    for &r in regs {
        let v = b
            .ins()
            .load(types::I64, MemFlags::trusted(), fb, (r * 8) as i32);
        b.def_var(actx.vars[r], v);
    }
}

/// Generic cross-function `Call` — the callee is a class (`new`) or any
/// non-int-contract value, so it can't use the clif→clif fast IC. Dispatch
/// through `clif_call_fallback` (which runs the callee, possibly a
/// constructor, in the interpreter and can therefore trigger a GC): flush
/// live heap refs to their home slots first, box the callee+args, call, then
/// reload. Result is unboxed to int only when the register meta proves it.
pub(super) fn emit_generic_call(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[varn_types::register_meta::RegisterMeta],
    code: &[u16],
    ip: usize,
) -> Result<(), String> {
    let w1 = code[ip + 1];
    let w2 = code[ip + 2];
    let dest = (w1 >> 8) as usize;
    let callee_reg = (w1 & 0xFF) as usize;
    let argc = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;
    if argc > 4 {
        return Err("clif: generic call arity > 4".into());
    }

    let callee = box_or_pass(b, actx.vars, state, callee_reg);
    let arg_vals: Vec<cranelift_codegen::ir::Value> = (0..argc)
        .map(|i| box_or_pass(b, actx.vars, state, arg_start + i))
        .collect();

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let argc_v = b.ins().iconst(types::I64, argc as i64);
    let zero = b.ins().iconst(types::I64, 0);
    let mut args = vec![actx.exec_ctx, callee, argc_v];
    for i in 0..4 {
        args.push(if i < argc { arg_vals[i] } else { zero });
    }
    let res = call_helper(b, actx.cc, actx.helpers.clif_call_fallback, &args);

    reload_boxed(b, actx, &regs);

    if meta.get(dest).map_or(false, |m| m.kind == varn_types::register_meta::SlotKind::Int) {
        let s = b.ins().ishl_imm(res, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(actx.vars[dest], un);
    } else {
        b.def_var(actx.vars[dest], res);
    }
    Ok(())
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
        &[actx.exec_ctx, actx.closure, obj, ni, ci, de, ipv],
    );

    reload_boxed(b, actx, &regs);

    b.def_var(actx.vars[dest], res);
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

    reload_boxed(b, actx, &regs);
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
        &[actx.exec_ctx, start_v, count_v],
    );
    b.def_var(actx.vars[dest], res);
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
    call_helper_void(b, actx.cc, actx.helpers.array_push, &[actx.exec_ctx, arr, val]);
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

    reload_boxed(b, actx, &regs);

    b.def_var(actx.vars[dest], res);
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
    meta: &[RegisterMeta],
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

    let fb = frame_base_addr(b, actx);
    for r in dest..dest + total {
        store_home(b, actx, state, fb, r);
    }
    let args_start = b.ins().iadd_imm(actx.base, dest as i64);
    let op_id_v = b.ins().iconst(types::I64, op_id as i64);
    let total_v = b.ins().iconst(types::I64, total as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.jit_call_native_op,
        &[actx.exec_ctx, op_id_v, args_start, total_v],
    );
    if meta.get(dest).map_or(false, |m| m.kind == varn_types::register_meta::SlotKind::Int) {
        let s = b.ins().ishl_imm(res, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(actx.vars[dest], un);
    } else {
        b.def_var(actx.vars[dest], res);
    }
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
        &[actx.exec_ctx, actx.closure, start_v, shape_v],
    );
    b.def_var(actx.vars[dest], res);
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

    let ip_v = b.ins().iconst(types::I64, ip as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.make_enum_variant,
        &[actx.exec_ctx, ip_v],
    );

    reload_boxed(b, actx, &regs);
    b.def_var(actx.vars[dest], res);
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

    reload_boxed(b, actx, &regs);

    if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else if meta.get(dest).map_or(false, |m| m.kind == SlotKind::Int) {
        let s = b.ins().ishl_imm(res, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(actx.vars[dest], un);
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

    reload_boxed(b, actx, &regs);
    b.def_var(actx.vars[dest], res);
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

    reload_boxed(b, actx, &regs);

    if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else if meta.get(dest).map_or(false, |m| m.kind == SlotKind::Int) {
        let s = b.ins().ishl_imm(res, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(actx.vars[dest], un);
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

    reload_boxed(b, actx, &regs);

    if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else if meta.get(dest).map_or(false, |m| m.kind == SlotKind::Int) {
        let s = b.ins().ishl_imm(res, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(actx.vars[dest], un);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}
