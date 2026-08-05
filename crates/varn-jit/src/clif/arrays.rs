//! Array-access lowering for CLIF: `ArrayLength`, `ArrayGetIndex`,
//! `ArraySetIndex`. Each resolves the boxed receiver to its payload pointer
//! (inline fast path via `emit::cached_payload`, generic helper on the slow
//! path) and reads/writes an element. In an allocating function the payload
//! resolve is non-`readonly` (`!has_alloc`) so a back-edge safepoint's GC
//! can't leave a hoisted, stale resolve; the loop payload cache is disabled
//! there too. Split out of `lower.rs` for the file-size governance limit.
//!
//! # Unboxed element buffers
//!
//! An array's elements live in one of three reprs (`ArrayRepr`): `Boxed`
//! (`Vec<VmValue>`), `I64` (`Vec<i64>`) or `F64` (`Vec<f64>`). All three put
//! their data pointer and length at the same offsets, and all three use 8-byte
//! elements, so the resolve, the length load and the address arithmetic are
//! shared; only the element load/store differs.
//!
//! The reprs are what makes the typed pipeline pay off end to end: reading
//! `Array<int>` into an `Int` register is a bare `i64` load — no NaN-box, no
//! sign-extend — and writing one is a bare `i64` store, with no write barrier
//! because a raw number is never a heap reference. The same holds for
//! `Array<float>` in an `F64` register.
//!
//! The repr is a runtime property, so each access branches on the
//! discriminant (`emit::array_disc`, re-read per access — see its docs on why
//! it must not be folded into the cached resolve): the arm matching the
//! register's representation loads raw, the `Boxed` arm converts, and anything
//! else falls to the generic helper.

use std::collections::HashMap;

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags, Type};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::register_meta::RegisterMeta;

use super::emit::{
    array_disc, box_bool, box_f64, box_int, cached_payload, call_helper, find_cache, meta_is_float,
    state_meta_int, unbox_f64_coerce, use_boxed, use_f64, use_int, wrap_i48,
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
    pub regions: &'a [super::emit::Region],
    pub cache_vars: &'a HashMap<(usize, usize), super::emit::RegionCache>,
    pub register_meta: &'a [RegisterMeta],
    pub has_alloc: bool,
}

/// How the register on the value side of an element access represents its
/// value, and therefore which `ArrayRepr` it can exchange elements with raw.
#[derive(Clone, Copy, PartialEq, Debug)]
enum ElemRepr {
    /// NaN-boxed `VmValue` bits — pairs raw with `ArrayRepr::Boxed` (disc 0).
    Boxed,
    /// Unboxed i48-in-`i64` — pairs raw with `ArrayRepr::I64` (disc 1).
    Int,
    /// Unboxed `f64` — pairs raw with `ArrayRepr::F64` (disc 2).
    Float,
}

impl ElemRepr {
    /// The `ArrayRepr` discriminant this representation reads/writes raw.
    fn disc(self) -> i64 {
        match self {
            ElemRepr::Boxed => 0,
            ElemRepr::Int => 1,
            ElemRepr::Float => 2,
        }
    }

    /// The Cranelift type a value in this representation has.
    fn ty(self) -> Type {
        match self {
            ElemRepr::Float => types::F64,
            _ => types::I64,
        }
    }
}

/// The representation register `r` holds, from the register meta (which is
/// what declares an `Int`/`F64` Variable in the first place).
fn reg_repr(meta: &[RegisterMeta], r: usize) -> ElemRepr {
    if meta_is_float(meta, r) {
        ElemRepr::Float
    } else if state_meta_int(meta, r) {
        ElemRepr::Int
    } else {
        ElemRepr::Boxed
    }
}

/// Element byte address: every repr stores 8-byte elements, so this is shared.
fn elem_addr(
    b: &mut FunctionBuilder,
    payload: cranelift_codegen::ir::Value,
    key: cranelift_codegen::ir::Value,
    lay: &crate::JitArrayLayout,
) -> cranelift_codegen::ir::Value {
    let data = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        payload,
        (16 + lay.elems_ptr_off) as i32,
    );
    let off = b.ins().ishl_imm(key, 3);
    b.ins().iadd(data, off)
}

/// Load the element at `addr` out of an array whose repr is `from`, converted
/// into the `want` representation.
fn load_elem_as(
    b: &mut FunctionBuilder,
    addr: cranelift_codegen::ir::Value,
    from: ElemRepr,
    want: ElemRepr,
) -> cranelift_codegen::ir::Value {
    let raw = b.ins().load(from.ty(), MemFlags::trusted(), addr, 0);
    convert(b, raw, from, want)
}

/// Reinterpret a value that is in representation `from` as representation
/// `want`. Every conversion here is exact: `box_int`/`wrap_i48` round-trip an
/// i48 payload, and `box_f64` reproduces `VmValue::from_f64` (including its
/// quiet-NaN canonicalization), so an element read back through a typed repr
/// is bit-identical to the `VmValue` that was stored.
fn convert(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
    from: ElemRepr,
    want: ElemRepr,
) -> cranelift_codegen::ir::Value {
    match (from, want) {
        (a, w) if a == w => v,
        // Boxed VmValue bits → unboxed. The float case coerces, since a boxed
        // int can legitimately sit in a float-typed slot (`takesFloat(5)`).
        (ElemRepr::Boxed, ElemRepr::Int) => wrap_i48(b, v),
        (ElemRepr::Boxed, ElemRepr::Float) => unbox_f64_coerce(b, v),
        // Unboxed → boxed VmValue bits.
        (ElemRepr::Int, ElemRepr::Boxed) => box_int(b, v),
        (ElemRepr::Float, ElemRepr::Boxed) => box_f64(b, v),
        // Cross-typed (an I64 array read into an F64 register, or vice versa)
        // is never emitted: those arms route to the generic helper instead, so
        // the interpreter's own widening rules stay the single source of truth.
        (a, w) => unreachable!("clif: no direct {a:?} -> {w:?} element conversion"),
    }
}

/// `ArrayLength first_reg, src` — a heap array's length is a single Vec-len
/// load, unboxed; non-array receivers fall to the generic helper.
///
/// No repr branch: `elems_len_off` lands on the `Vec` length word of every
/// variant (checked at startup by `Heap::jit_array_layout`'s typed probe), so
/// one load serves all three.
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
    let len = match cache.and_then(|c| c.view) {
        // Hoisted length: `.length` inside a read-only region is one variable
        // read (see `RegionCache`).
        Some(view) => {
            let data = b.use_var(view[0]);
            let resolved = b.ins().icmp_imm(IntCC::NotEqual, data, 0);
            let live = b.create_block();
            b.ins().brif(resolved, live, &[], slow, &[]);
            b.switch_to_block(live);
            b.use_var(view[1])
        }
        None => {
            let payload = cached_payload(
                b,
                c.exec_ctx,
                obj,
                &c.helpers.array_layout,
                c.helpers.heap_field_offset,
                slow,
                cache.map(|c| c.payload),
                !c.has_alloc,
            );
            b.ins().load(
                types::I64,
                MemFlags::trusted(),
                payload,
                (16 + c.helpers.array_layout.elems_len_off) as i32,
            )
        }
    };
    b.ins().jump(merge, &[len.into()]);
    // slow: generic helper returns a boxed int; unbox.
    b.switch_to_block(slow);
    let boxed = call_helper(b, c.cc, c.helpers.array_length, &[c.exec_ctx, obj]);
    let un = wrap_i48(b, boxed);
    b.ins().jump(merge, &[un.into()]);
    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    b.def_var(c.vars[first_reg], res);
    Ok(())
}

/// `ArrayGetIndex first_reg, obj, key` — bounds-checked inline element load;
/// out-of-bounds / non-array falls to the generic (allocation-free) helper.
///
/// The destination register's representation picks which repr is served raw:
/// an `Int` register reads an `I64` array with a bare `i64` load (no box, no
/// unbox), an `F64` register reads an `F64` array with a bare `f64` load. The
/// `Boxed` arm is always emitted too, so an array that never specialized —
/// or migrated back — keeps its inline path. The remaining cross-typed
/// combination goes to the helper.
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
    let want = reg_repr(c.register_meta, first_reg);

    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, want.ty());
    let cache = find_cache(c.regions, c.cache_vars, ip, obj_r);
    let lay = &c.helpers.array_layout;

    // Repr-validated fast path: the preheader already checked disc == expected
    // and zeroed `data` on mismatch. `data != 0` guarantees both "resolved"
    // AND "disc matches expected repr", so the access is just:
    //   sentinel check → bounds check → raw load
    // No repr branching at all — 2 branches instead of 4.
    let repr_validated = cache.is_some_and(|c| c.repr_validated_disc.is_some());
    if repr_validated {
        if let Some(view) = cache.and_then(|c| c.view) {
            let data = b.use_var(view[0]);
            let resolved = b.ins().icmp_imm(IntCC::NotEqual, data, 0);
            let live = b.create_block();
            b.ins().brif(resolved, live, &[], slow, &[]);
            b.switch_to_block(live);
            let len = b.use_var(view[1]);

            // Bounds check (unsigned also rejects negative keys).
            let oob = b.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, key, len);
            let inb = b.create_block();
            b.ins().brif(oob, slow, &[], inb, &[]);
            b.switch_to_block(inb);

            // Direct raw load — repr is guaranteed by the preheader.
            let off = b.ins().ishl_imm(key, 3);
            let addr = b.ins().iadd(data, off);
            let v = load_elem_as(b, addr, want, want);
            b.ins().jump(merge, &[v.into()]);

            // slow: generic helper (always correct, handles every repr).
            b.switch_to_block(slow);
            let boxed_key = box_int(b, key);
            let r = call_helper(
                b,
                c.cc,
                c.helpers.jit_array_get_fast,
                &[c.exec_ctx, obj, boxed_key],
            );
            let r = convert(b, r, ElemRepr::Boxed, want);
            b.ins().jump(merge, &[r.into()]);

            b.switch_to_block(merge);
            let res = b.block_params(merge)[0];
            b.def_var(c.vars[first_reg], res);
            return Ok(());
        }
    }

    // Standard path: resolve payload, bounds-check, repr-branch per access.
    // A read-only receiver in an allocation-free region had its data pointer,
    // length and repr hoisted into the region's preheader, so the whole
    // resolve chain and all three loads collapse to variable reads here. The
    // `data == 0` sentinel means the preheader's guard chain rejected the
    // receiver; the generic helper answers for it, as it does for every other
    // rejection.
    let (data, len, disc) = match cache.and_then(|c| c.view) {
        Some(view) => {
            let data = b.use_var(view[0]);
            let resolved = b.ins().icmp_imm(IntCC::NotEqual, data, 0);
            let live = b.create_block();
            b.ins().brif(resolved, live, &[], slow, &[]);
            b.switch_to_block(live);
            (data, b.use_var(view[1]), b.use_var(view[2]))
        }
        None => {
            let payload = cached_payload(
                b,
                c.exec_ctx,
                obj,
                lay,
                c.helpers.heap_field_offset,
                slow,
                cache.map(|c| c.payload),
                !c.has_alloc,
            );
            let data = b.ins().load(
                types::I64,
                MemFlags::trusted(),
                payload,
                (16 + lay.elems_ptr_off) as i32,
            );
            let len = b.ins().load(
                types::I64,
                MemFlags::trusted(),
                payload,
                (16 + lay.elems_len_off) as i32,
            );
            (data, len, array_disc(b, payload, lay))
        }
    };

    // Unsigned compare also rejects negative keys.
    let oob = b.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, key, len);
    let inb = b.create_block();
    b.ins().brif(oob, slow, &[], inb, &[]);
    b.switch_to_block(inb);

    let off = b.ins().ishl_imm(key, 3);
    let addr = b.ins().iadd(data, off);
    // Raw arm first (the expected repr for this destination), then the Boxed
    // arm, then the helper. When the destination is already boxed the two
    // coincide and only one arm is emitted.
    if want != ElemRepr::Boxed {
        let raw_blk = b.create_block();
        let not_raw = b.create_block();
        let is_raw = b.ins().icmp_imm(IntCC::Equal, disc, want.disc());
        b.ins().brif(is_raw, raw_blk, &[], not_raw, &[]);
        b.switch_to_block(raw_blk);
        let v = load_elem_as(b, addr, want, want);
        b.ins().jump(merge, &[v.into()]);
        b.switch_to_block(not_raw);
    }
    let boxed_blk = b.create_block();
    let is_boxed = b.ins().icmp_imm(IntCC::Equal, disc, 0);
    b.ins().brif(is_boxed, boxed_blk, &[], slow, &[]);
    b.switch_to_block(boxed_blk);
    let raw = b.ins().load(types::I64, MemFlags::trusted(), addr, 0);
    if want == ElemRepr::Int {
        let tag_mask = b.ins().iconst(types::I64, 0x7FFF_0000_0000_0000i64);
        let int_expect = b.ins().iconst(types::I64, 0x7FFC_0000_0000_0000i64);
        let masked = b.ins().band(raw, tag_mask);
        let is_int_elem = b.ins().icmp(IntCC::Equal, masked, int_expect);
        let int_blk = b.create_block();
        b.ins().brif(is_int_elem, int_blk, &[], slow, &[]);
        b.switch_to_block(int_blk);
        let v = wrap_i48(b, raw);
        b.ins().jump(merge, &[v.into()]);
    } else if want == ElemRepr::Float {
        let num_blk = b.create_block();
        let qnan = b.ins().iconst(types::I64, 0x7FF8_0000_0000_0000i64);
        let masked = b.ins().band(raw, qnan);
        let is_f64_elem = b.ins().icmp(IntCC::NotEqual, masked, qnan);
        let tag_mask = b.ins().iconst(types::I64, 0x7FFF_0000_0000_0000i64);
        let int_expect = b.ins().iconst(types::I64, 0x7FFC_0000_0000_0000i64);
        let masked_int = b.ins().band(raw, tag_mask);
        let is_int_elem = b.ins().icmp(IntCC::Equal, masked_int, int_expect);
        let is_num = b.ins().bor(is_f64_elem, is_int_elem);
        b.ins().brif(is_num, num_blk, &[], slow, &[]);
        b.switch_to_block(num_blk);
        let v = unbox_f64_coerce(b, raw);
        b.ins().jump(merge, &[v.into()]);
    } else {
        b.ins().jump(merge, &[raw.into()]);
    }

    // slow: same generic helper as the template (returns null out of bounds;
    // allocates nothing). It answers for every repr, so a cross-typed access
    // lands here rather than bailing the function out of clif.
    b.switch_to_block(slow);
    let boxed_key = box_int(b, key);
    let r = call_helper(
        b,
        c.cc,
        c.helpers.jit_array_get_fast,
        &[c.exec_ctx, obj, boxed_key],
    );
    let r = convert(b, r, ElemRepr::Boxed, want);
    b.ins().jump(merge, &[r.into()]);

    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    b.def_var(c.vars[first_reg], res);
    Ok(())
}

/// `ArraySetIndex obj(=first_reg), idx, val` — bounds-checked inline store on
/// a proven-numeric value (`K::Int` / `K::Float`). An unboxed number is never
/// a heap reference, so no write barrier is needed on any of these arms: an
/// `I64` array takes a bare `i64` store, an `F64` array a bare `f64` store,
/// and a `Boxed` array takes the boxed value. Append/OOB/non-array/cross-typed
/// falls to the helper.
///
/// Any other value kind — proven boxed/bool, or a flow-merge the dataflow
/// couldn't resolve to one representation — skips the inline path entirely:
/// storing a value that might be a heap reference needs the write barrier only
/// the helper carries, so this one instruction routes straight to the same
/// helper instead of bailing the whole function out of clif.
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

    let src = match state[val_r] {
        K::Int => ElemRepr::Int,
        K::Float => ElemRepr::Float,
        _ => ElemRepr::Boxed,
    };
    if src == ElemRepr::Boxed {
        // Unproven kind: box it (bool needs re-tagging; boxed/global kinds
        // are already the right bits — `use_boxed` still bails on a truly
        // unrepresentable merge, same as it would for any other read) and
        // hand the whole store to the generic helper, no inline attempt.
        let val = match state[val_r] {
            K::Bool => {
                let raw = b.use_var(c.vars[val_r]);
                box_bool(b, raw)
            }
            _ => use_boxed(b, c.vars, state, val_r)?,
        };
        let boxed_key = box_int(b, key);
        let _ = call_helper(
            b,
            c.cc,
            c.helpers.jit_array_set_fast,
            &[c.exec_ctx, obj, boxed_key, val],
        );
        return Ok(());
    }
    let raw_val = if src == ElemRepr::Float {
        use_f64(b, c.vars, state, val_r)?
    } else {
        use_int(b, c.vars, state, val_r)?
    };

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
        cache.map(|c| c.payload),
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

    let disc = array_disc(b, payload, lay);
    let addr = elem_addr(b, payload, key, lay);
    // Raw arm (matching typed repr), then the Boxed arm, then the helper.
    let raw_blk = b.create_block();
    let not_raw = b.create_block();
    let is_raw = b.ins().icmp_imm(IntCC::Equal, disc, src.disc());
    b.ins().brif(is_raw, raw_blk, &[], not_raw, &[]);
    b.switch_to_block(raw_blk);
    b.ins().store(MemFlags::trusted(), raw_val, addr, 0);
    b.ins().jump(merge, &[]);
    b.switch_to_block(not_raw);

    let boxed_blk = b.create_block();
    let is_boxed = b.ins().icmp_imm(IntCC::Equal, disc, 0);
    b.ins().brif(is_boxed, boxed_blk, &[], slow, &[]);
    b.switch_to_block(boxed_blk);
    let boxed_val = convert(b, raw_val, src, ElemRepr::Boxed);
    b.ins().store(MemFlags::trusted(), boxed_val, addr, 0);
    b.ins().jump(merge, &[]);

    // slow: append/OOB/non-array/cross-typed semantics live in the helper (it
    // grows the Rust vec and migrates the repr when needed — no VM-heap
    // allocation, no GC).
    b.switch_to_block(slow);
    let boxed_key = box_int(b, key);
    let boxed_val = convert(b, raw_val, src, ElemRepr::Boxed);
    let _ = call_helper(
        b,
        c.cc,
        c.helpers.jit_array_set_fast,
        &[c.exec_ctx, obj, boxed_key, boxed_val],
    );
    b.ins().jump(merge, &[]);
    b.switch_to_block(merge);
    Ok(())
}
