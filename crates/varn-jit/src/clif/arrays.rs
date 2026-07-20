//! Array-access lowering for CLIF: `ArrayLength`, `ArrayGetIndex`,
//! `ArraySetIndex`. Each resolves the boxed receiver to its payload pointer
//! (inline fast path via `emit::cached_payload`, generic helper on the slow
//! path) and reads/writes an element. In an allocating function the payload
//! resolve is non-`readonly` (`!has_alloc`) so a back-edge safepoint's GC
//! can't leave a hoisted, stale resolve; the loop payload cache is disabled
//! there too. Split out of `lower.rs` for the file-size governance limit.

use std::collections::HashMap;

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::register_meta::RegisterMeta;

use super::emit::{
    box_int, cached_payload, call_helper, find_cache, state_meta_int, use_boxed, use_int,
};
use super::kinds::K;
use crate::JitHelpers;

/// Shared context for the array arms — the immutable references the payload
/// resolve and helper calls need.
pub(super) struct ArrCtx<'a> {
    pub vars: &'a [Variable],
    pub helpers: &'a JitHelpers,
    pub cc: CallConv,
    pub exec_ctx: cranelift_codegen::ir::Value,
    pub regions: &'a [(usize, usize, Vec<usize>)],
    pub cache_vars: &'a HashMap<(usize, usize), Variable>,
    pub register_meta: &'a [RegisterMeta],
    pub has_alloc: bool,
}

/// `ArrayLength first_reg, src` — a heap array's length is a single Vec-len
/// load, unboxed; non-array receivers fall to the generic helper.
pub(super) fn emit_array_length(
    b: &mut FunctionBuilder,
    c: &ArrCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let src = (code[ip + 1] >> 8) as usize;
    let obj = use_boxed(b, c.vars, state, src)?;
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I64); // unboxed len
    let cache = find_cache(c.regions, c.cache_vars, ip, src);
    let payload = cached_payload(
        b,
        c.exec_ctx,
        obj,
        &c.helpers.array_layout,
        c.helpers.heap_field_offset,
        slow,
        cache,
        !c.has_alloc,
    );
    let len = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        payload,
        (16 + c.helpers.array_layout.elems_len_off) as i32,
    );
    b.ins().jump(merge, &[len.into()]);
    // slow: generic helper returns a boxed int; unbox.
    b.switch_to_block(slow);
    let boxed = call_helper(b, c.cc, c.helpers.array_length, &[c.exec_ctx, obj]);
    let s = b.ins().ishl_imm(boxed, 16);
    let un = b.ins().sshr_imm(s, 16);
    b.ins().jump(merge, &[un.into()]);
    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    b.def_var(c.vars[first_reg], res);
    Ok(())
}

/// `ArrayGetIndex first_reg, obj, key` — bounds-checked inline element load;
/// out-of-bounds / non-array falls to the generic (allocation-free) helper.
pub(super) fn emit_array_get_index(
    b: &mut FunctionBuilder,
    c: &ArrCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let w1 = code[ip + 1];
    let obj_r = (w1 >> 8) as usize;
    let key_r = (w1 & 0xFF) as usize;
    let obj = use_boxed(b, c.vars, state, obj_r)?;
    let key = use_int(b, c.vars, state, key_r)?;
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I64); // boxed element
    let cache = find_cache(c.regions, c.cache_vars, ip, obj_r);
    let payload = cached_payload(
        b,
        c.exec_ctx,
        obj,
        &c.helpers.array_layout,
        c.helpers.heap_field_offset,
        slow,
        cache,
        !c.has_alloc,
    );
    let lay = &c.helpers.array_layout;
    let len = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        payload,
        (16 + lay.elems_len_off) as i32,
    );
    // Unsigned compare also rejects negative keys.
    let oob = b.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, key, len);
    let inb = b.create_block();
    b.ins().brif(oob, slow, &[], inb, &[]);
    b.switch_to_block(inb);
    let data = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        payload,
        (16 + lay.elems_ptr_off) as i32,
    );
    let off = b.ins().ishl_imm(key, 3);
    let addr = b.ins().iadd(data, off);
    let elem = b.ins().load(types::I64, MemFlags::trusted(), addr, 0);
    b.ins().jump(merge, &[elem.into()]);
    // slow: same generic helper as the template (returns null out of bounds;
    // allocates nothing).
    b.switch_to_block(slow);
    let boxed_key = box_int(b, key);
    let r = call_helper(
        b,
        c.cc,
        c.helpers.jit_array_get_fast,
        &[c.exec_ctx, obj, boxed_key],
    );
    b.ins().jump(merge, &[r.into()]);
    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    if state_meta_int(c.register_meta, first_reg) {
        let s = b.ins().ishl_imm(res, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(c.vars[first_reg], un);
    } else {
        b.def_var(c.vars[first_reg], res);
    }
    Ok(())
}

/// `ArraySetIndex obj(=first_reg), idx, val` — bounds-checked inline int
/// store; append/OOB/non-array falls to the helper. Only int stores admitted
/// (a boxed int is never a heap ref, so no write barrier needed).
pub(super) fn emit_array_set_index(
    b: &mut FunctionBuilder,
    c: &ArrCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let w1 = code[ip + 1];
    let idx_r = (w1 >> 8) as usize;
    let val_r = (w1 & 0xFF) as usize;
    let obj_r = first_reg;
    let obj = use_boxed(b, c.vars, state, obj_r)?;
    let key = use_int(b, c.vars, state, idx_r)?;
    if state[val_r] != K::Int {
        return Err("clif: non-int array store".into());
    }
    let raw_val = b.use_var(c.vars[val_r]);
    let val = box_int(b, raw_val);
    let slow = b.create_block();
    let merge = b.create_block();
    let cache = find_cache(c.regions, c.cache_vars, ip, obj_r);
    let payload = cached_payload(
        b,
        c.exec_ctx,
        obj,
        &c.helpers.array_layout,
        c.helpers.heap_field_offset,
        slow,
        cache,
        !c.has_alloc,
    );
    let lay = &c.helpers.array_layout;
    let len = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        payload,
        (16 + lay.elems_len_off) as i32,
    );
    let oob = b.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, key, len);
    let inb = b.create_block();
    b.ins().brif(oob, slow, &[], inb, &[]);
    b.switch_to_block(inb);
    let data = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        payload,
        (16 + lay.elems_ptr_off) as i32,
    );
    let off = b.ins().ishl_imm(key, 3);
    let addr = b.ins().iadd(data, off);
    b.ins().store(MemFlags::trusted(), val, addr, 0);
    b.ins().jump(merge, &[]);
    // slow: append/OOB/non-array semantics live in the helper (grows the Rust
    // vec — no VM-heap allocation, no GC).
    b.switch_to_block(slow);
    let boxed_key = box_int(b, key);
    let _ = call_helper(
        b,
        c.cc,
        c.helpers.jit_array_set_fast,
        &[c.exec_ctx, obj, boxed_key, val],
    );
    b.ins().jump(merge, &[]);
    b.switch_to_block(merge);
    Ok(())
}
