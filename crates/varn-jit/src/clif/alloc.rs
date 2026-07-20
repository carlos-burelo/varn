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

use super::emit::{box_int, call_helper, call_helper_void};
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
fn frame_base_addr(b: &mut FunctionBuilder, actx: &AllocCtx) -> cranelift_codegen::ir::Value {
    let sp = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.stack_data_offset as i32,
    );
    let base_bytes = b.ins().ishl_imm(actx.base, 3);
    b.ins().iadd(sp, base_bytes)
}

/// Read a register as boxed VmValue bits: an int is re-tagged, boxed kinds
/// pass through. Used both for helper arguments and home-slot flushes so a
/// well-formed VmValue always reaches the collector / the runtime.
fn box_or_pass(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    r: usize,
) -> cranelift_codegen::ir::Value {
    let raw = b.use_var(actx.vars[r]);
    if state[r] == K::Int {
        box_int(b, raw)
    } else {
        raw
    }
}

/// Store `reg`'s current value into its `ctx.stack` home slot.
fn store_home(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    fb: cranelift_codegen::ir::Value,
    reg: usize,
) {
    let v = box_or_pass(b, actx, state, reg);
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
    // Flush every register that could hold a heap reference to its home slot
    // so the collector roots (and rewrites) it. Ints are GC-irrelevant and
    // stay in registers.
    let boxed: Vec<usize> = (0..actx.nregs).filter(|&r| is_boxed_kind(state[r])).collect();
    let fb = frame_base_addr(b, actx);
    for &r in &boxed {
        store_home(b, actx, state, fb, r);
    }
    call_helper_void(b, actx.cc, h.gc_safepoint, &[actx.exec_ctx]);
    // The collection may have grown/moved `ctx.stack`; recompute the base.
    let fb2 = frame_base_addr(b, actx);
    for &r in &boxed {
        let v = b
            .ins()
            .load(types::I64, MemFlags::trusted(), fb2, (r * 8) as i32);
        b.def_var(actx.vars[r], v);
    }
    b.ins().jump(cont, &[]);
    b.switch_to_block(cont);
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
    let arr = box_or_pass(b, actx, state, arr_r);
    let val = box_or_pass(b, actx, state, val_r);
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
    let a = box_or_pass(b, actx, state, a_r);
    let bb = box_or_pass(b, actx, state, b_r);
    let res = call_helper(b, actx.cc, actx.helpers.str_concat, &[actx.exec_ctx, a, bb]);
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
    b.def_var(actx.vars[dest], res);
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

/// Generic binary op `dest, a, b` dispatched through a runtime helper
/// (`Add`/`Sub`/`Mul`/`Div` on values of not-statically-proven-int type —
/// e.g. feeding an untyped object field). Operands pass by value; `Add`
/// allocates on the string-concat path. Result is boxed bits.
pub(super) fn emit_binop(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    helper: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let a_r = (code[ip + 1] >> 8) as usize;
    let b_r = (code[ip + 1] & 0xFF) as usize;
    let a = box_or_pass(b, actx, state, a_r);
    let bb = box_or_pass(b, actx, state, b_r);
    let res = call_helper(b, actx.cc, helper, &[actx.exec_ctx, a, bb]);
    b.def_var(actx.vars[dest], res);
}
