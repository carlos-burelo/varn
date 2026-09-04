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

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::register_meta::RegisterMeta;

use super::alloc::{box_or_load_home, def_result, AllocCtx};
use super::emit::{
    array_disc, box_bool, box_f64, box_int, cached_payload, call_helper_void, meta_is_float,
    state_meta_int, unbox_f64_coerce, use_boxed, use_f64, use_int, wrap_i48,
};
use super::kinds::K;
use crate::JitHelpers;

/// Shared context for the array arms — the immutable references the payload
/// resolve and helper calls need.
pub(crate) struct ArrCtx<'a> {
    pub vars: &'a [Variable],
    pub helpers: &'a JitHelpers,
    pub cc: CallConv,
    pub exec_ctx: cranelift_codegen::ir::Value,
    pub loops: super::emit::LoopCaches<'a>,
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

fn convert(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
    from: ElemRepr,
    want: ElemRepr,
) -> cranelift_codegen::ir::Value {
    match (from, want) {
        (a, w) if a == w => v,
        (ElemRepr::Boxed, ElemRepr::Int) => wrap_i48(b, v),
        (ElemRepr::Boxed, ElemRepr::Float) => unbox_f64_coerce(b, v),
        (ElemRepr::Int, ElemRepr::Boxed) => box_int(b, v),
        (ElemRepr::Float, ElemRepr::Boxed) => box_f64(b, v),
        (a, w) => unreachable!("clif: no direct {a:?} -> {w:?} element conversion"),
    }
}

pub(super) fn emit_array_length(
    b: &mut FunctionBuilder,
    c: &ArrCtx,
    actx: Option<&AllocCtx>,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let src = (code[ip + 1] >> 8) as usize;
    let obj = if let Some(actx) = actx {
        box_or_load_home(b, actx, state, src)
    } else {
        use_boxed(b, c.vars, state, src)?
    };
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I64); // unboxed len

    let cache = c.loops.array(ip, src);
    let len = match cache.and_then(|c| c.view) {
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
    let (obj_tag, obj_payload) = if b.func.dfg.value_type(obj) == types::I128 {
        b.ins().isplit(obj)
    } else {
        (
            b.ins()
                .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64),
            obj,
        )
    };
    call_helper_void(
        b,
        c.cc,
        c.helpers.array_length,
        &[c.exec_ctx, obj_tag, obj_payload],
    );
    let boxed = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        c.exec_ctx,
        c.helpers.jit_native_result_offset as i32,
    );
    let un = wrap_i48(b, boxed);
    b.ins().jump(merge, &[un.into()]);
    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    if let Some(actx) = actx {
        let boxed = box_int(b, res);
        def_result(b, actx, first_reg, boxed);
    } else {
        b.def_var(c.vars[first_reg], res);
    }
    Ok(())
}

pub(super) fn emit_array_get_index(
    b: &mut FunctionBuilder,
    c: &ArrCtx,
    actx: Option<&AllocCtx>,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let w1 = code[ip + 1];
    let obj_r = (w1 >> 8) as usize;
    let key_r = (w1 & 0xFF) as usize;
    let obj = if let Some(actx) = actx {
        box_or_load_home(b, actx, state, obj_r)
    } else {
        use_boxed(b, c.vars, state, obj_r)?
    };
    let key = use_int(b, c.vars, state, key_r)?;
    let want = reg_repr(c.register_meta, first_reg);

    let slow = b.create_block();
    b.set_cold_block(slow);
    let merge = b.create_block();
    let merge_ty = match want {
        ElemRepr::Int => types::I64,
        ElemRepr::Float => types::F64,
        ElemRepr::Boxed => types::I128,
    };
    b.append_block_param(merge, merge_ty);

    let cache = c.loops.array(ip, obj_r);
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
                &c.helpers.array_layout,
                c.helpers.heap_field_offset,
                slow,
                cache.map(|c| c.payload),
                !c.has_alloc,
            );
            let lay = &c.helpers.array_layout;
            let m = MemFlags::trusted();
            let data = b
                .ins()
                .load(types::I64, m, payload, (16 + lay.elems_ptr_off) as i32);
            let len = b
                .ins()
                .load(types::I64, m, payload, (16 + lay.elems_len_off) as i32);
            let disc = array_disc(b, payload, lay);
            (data, len, disc)
        }
    };

    let target_disc = want.disc();
    let skip_disc_check = cache.and_then(|ca| ca.repr_validated_disc) == Some(target_disc);
    let skip_bounds_check = cache.map(|ca| ca.bounds_guaranteed).unwrap_or(false);

    if !skip_bounds_check {
        let in_bounds = b.ins().icmp(IntCC::UnsignedLessThan, key, len);
        let hit = b.create_block();
        b.ins().brif(in_bounds, hit, &[], slow, &[]);
        b.switch_to_block(hit);
    }

    let elem_ty = match want {
        ElemRepr::Int => types::I64,
        ElemRepr::Float => types::F64,
        ElemRepr::Boxed => types::I128,
    };
    let scale = if want == ElemRepr::Boxed { 4 } else { 3 };
    let off = b.ins().ishl_imm(key, scale);
    let addr = b.ins().iadd(data, off);

    if skip_disc_check {
        let v = b.ins().load(elem_ty, MemFlags::trusted(), addr, 0);
        b.ins().jump(merge, &[v.into()]);
    } else {
        let matched = b.create_block();
        let boxed_arm = b.create_block();

        let is_match = b.ins().icmp_imm(IntCC::Equal, disc, target_disc);
        b.ins().brif(is_match, matched, &[], boxed_arm, &[]);

        b.switch_to_block(matched);
        let v = b.ins().load(elem_ty, MemFlags::trusted(), addr, 0);
        b.ins().jump(merge, &[v.into()]);

        b.switch_to_block(boxed_arm);
        let is_boxed = b.ins().icmp_imm(IntCC::Equal, disc, 0);
        let do_boxed = b.create_block();
        b.ins().brif(is_boxed, do_boxed, &[], slow, &[]);

        b.switch_to_block(do_boxed);
        let off_b = b.ins().ishl_imm(key, 4);
        let addr_b = b.ins().iadd(data, off_b);
        let raw = b.ins().load(types::I128, MemFlags::trusted(), addr_b, 0);
        let v = convert(b, raw, ElemRepr::Boxed, want);
        b.ins().jump(merge, &[v.into()]);
    }

    b.switch_to_block(slow);
    let boxed_key = box_int(b, key);
    let (obj_tag, obj_payload) = if b.func.dfg.value_type(obj) == types::I128 {
        b.ins().isplit(obj)
    } else {
        (
            b.ins()
                .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64),
            obj,
        )
    };
    let (key_tag, key_payload) = b.ins().isplit(boxed_key);
    call_helper_void(
        b,
        c.cc,
        c.helpers.jit_array_get_fast,
        &[c.exec_ctx, obj_tag, obj_payload, key_tag, key_payload],
    );
    let r = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        c.exec_ctx,
        c.helpers.jit_native_result_offset as i32,
    );
    let r = convert(b, r, ElemRepr::Boxed, want);
    b.ins().jump(merge, &[r.into()]);

    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    if let Some(actx) = actx {
        let boxed = match want {
            ElemRepr::Int => box_int(b, res),
            ElemRepr::Float => box_f64(b, res),
            ElemRepr::Boxed => res,
        };
        def_result(b, actx, first_reg, boxed);
    } else {
        let payload = if b.func.dfg.value_type(res) == types::I128 {
            let (_tag, payload) = b.ins().isplit(res);
            payload
        } else {
            res
        };
        b.def_var(c.vars[first_reg], payload);
    }
    Ok(())
}

pub(super) fn emit_array_set_index(
    b: &mut FunctionBuilder,
    c: &ArrCtx,
    actx: Option<&AllocCtx>,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let w1 = code[ip + 1];
    let obj_r = first_reg;
    let key_r = (w1 >> 8) as usize;
    let val_r = (w1 & 0xFF) as usize;

    let obj = if let Some(actx) = actx {
        box_or_load_home(b, actx, state, obj_r)
    } else {
        use_boxed(b, c.vars, state, obj_r)?
    };
    let key = use_int(b, c.vars, state, key_r)?;
    let src = reg_repr(c.register_meta, val_r);

    if src == ElemRepr::Boxed {
        let val = if let Some(actx) = actx {
            box_or_load_home(b, actx, state, val_r)
        } else {
            match state[val_r] {
                K::Int => {
                    let raw = b.use_var(c.vars[val_r]);
                    box_int(b, raw)
                }
                K::Bool => {
                    let raw = b.use_var(c.vars[val_r]);
                    box_bool(b, raw)
                }
                _ => {
                    let raw = use_boxed(b, c.vars, state, val_r)?;
                    let tag_v = b
                        .ins()
                        .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
                    b.ins().iconcat(tag_v, raw)
                }
            }
        };
        let boxed_key = box_int(b, key);
        let (obj_tag, obj_payload) = if b.func.dfg.value_type(obj) == types::I128 {
            b.ins().isplit(obj)
        } else {
            (
                b.ins()
                    .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64),
                obj,
            )
        };
        let (key_tag, key_payload) = b.ins().isplit(boxed_key);
        let (val_tag, val_payload) = b.ins().isplit(val);
        call_helper_void(
            b,
            c.cc,
            c.helpers.jit_array_set_fast,
            &[
                c.exec_ctx,
                obj_tag,
                obj_payload,
                key_tag,
                key_payload,
                val_tag,
                val_payload,
            ],
        );
        return Ok(());
    }

    let raw_val = if src == ElemRepr::Float {
        use_f64(b, c.vars, state, val_r)?
    } else {
        use_int(b, c.vars, state, val_r)?
    };

    let slow = b.create_block();
    b.set_cold_block(slow);
    let merge = b.create_block();
    let cache = c.loops.array(ip, obj_r);
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
    let in_bounds = b.ins().icmp(IntCC::UnsignedLessThan, key, len);
    let hit = b.create_block();
    b.ins().brif(in_bounds, hit, &[], slow, &[]);

    b.switch_to_block(hit);
    let data = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        payload,
        (16 + lay.elems_ptr_off) as i32,
    );
    let disc = array_disc(b, payload, lay);
    let matched = b.create_block();
    let boxed_arm = b.create_block();
    let target_disc = src.disc();
    let is_match = b.ins().icmp_imm(IntCC::Equal, disc, target_disc);
    b.ins().brif(is_match, matched, &[], boxed_arm, &[]);

    // 1. Matching repr: bare store into the buffer.
    b.switch_to_block(matched);
    let off = b.ins().ishl_imm(key, 3);
    let addr = b.ins().iadd(data, off);
    b.ins().store(MemFlags::trusted(), raw_val, addr, 0);
    b.ins().jump(merge, &[]);

    // 2. Boxed arm: the array holds `VmValue`s; store boxed value.
    b.switch_to_block(boxed_arm);
    let is_boxed = b.ins().icmp_imm(IntCC::Equal, disc, 0);
    let do_boxed = b.create_block();
    b.ins().brif(is_boxed, do_boxed, &[], slow, &[]);

    b.switch_to_block(do_boxed);
    let off_b = b.ins().ishl_imm(key, 4);
    let addr_b = b.ins().iadd(data, off_b);
    let boxed_val = match src {
        ElemRepr::Float => box_f64(b, raw_val),
        _ => box_int(b, raw_val),
    };
    b.ins().store(MemFlags::trusted(), boxed_val, addr_b, 0);
    b.ins().jump(merge, &[]);

    // 3. Fallback: out-of-bounds, cross-typed write, or non-array receiver.
    b.switch_to_block(slow);
    let boxed_val = if let Some(actx) = actx {
        box_or_load_home(b, actx, state, val_r)
    } else {
        match src {
            ElemRepr::Float => box_f64(b, raw_val),
            _ => box_int(b, raw_val),
        }
    };
    let boxed_key = box_int(b, key);
    let (obj_tag, obj_payload) = if b.func.dfg.value_type(obj) == types::I128 {
        b.ins().isplit(obj)
    } else {
        (
            b.ins()
                .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64),
            obj,
        )
    };
    let (key_tag, key_payload) = b.ins().isplit(boxed_key);
    let (val_tag, val_payload) = b.ins().isplit(boxed_val);
    call_helper_void(
        b,
        c.cc,
        c.helpers.jit_array_set_fast,
        &[
            c.exec_ctx,
            obj_tag,
            obj_payload,
            key_tag,
            key_payload,
            val_tag,
            val_payload,
        ],
    );
    b.ins().jump(merge, &[]);

    b.switch_to_block(merge);
    Ok(())
}
