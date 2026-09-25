//! Element access on a proven array with an `int` index, inline.
//!
//! An array's elements live in one of its `ArrayRepr`s; `Boxed` (`VmValue`s),
//! `I64` and `F64` are read and written here without a helper. The access
//! resolves the receiver to its payload (heap tag, generation, slot tag —
//! `emit::cached_payload`), checks the index against the length, and
//! branches on the representation, re-read per access because a write can
//! change it:
//!
//! * the representation matching the value's class (`I64` for an `int`,
//!   `F64` for a `float`) is a bare 8-byte load or store — a raw number is
//!   never a heap reference, so no write barrier;
//! * `Boxed` converts between the `VmValue` and the value's class; a boxed
//!   store of a scalar needs no barrier either;
//! * anything else — a narrow representation, a boxed store of a heap value
//!   (it needs the barrier), an index out of range, a receiver that is not an
//!   array after all — takes `jit_array_get_fast` / `jit_array_set_fast`,
//!   the runtime's own accessors.
//!
//! The buffer, length and representation come from the receiver's cached
//! view when one is set ([`super::views`]).

use cranelift_codegen::ir::{condcodes::IntCC, types, Block, InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

use super::super::emit::{
    array_disc, box_f64, box_int, cached_payload, call_helper_void, unbox_f64_coerce, unbox_int,
};
use super::{heap, load_value, Ctx, Out};

/// How a value side exchanges elements: raw with the matching
/// representation, converted with `Boxed`.
#[derive(Clone, Copy, PartialEq)]
enum Elem {
    Int,
    Float,
    /// Any other class: a whole `VmValue`.
    Boxed,
}

impl Elem {
    fn of(kind: SlotKind) -> Self {
        match kind {
            SlotKind::Int => Elem::Int,
            SlotKind::Float => Elem::Float,
            SlotKind::Bool | SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic => Elem::Boxed,
        }
    }

    /// The `ArrayRepr` discriminant read and written raw.
    fn disc(self) -> i64 {
        match self {
            Elem::Boxed => 0,
            Elem::Int => 1,
            Elem::Float => 2,
        }
    }

    fn clif_ty(self) -> types::Type {
        match self {
            Elem::Int => types::I64,
            Elem::Float => types::F64,
            Elem::Boxed => types::I128,
        }
    }

    fn from_boxed(self, b: &mut FunctionBuilder, v: Value) -> Value {
        match self {
            Elem::Int => unbox_int(b, v),
            Elem::Float => unbox_f64_coerce(b, v),
            Elem::Boxed => v,
        }
    }

    fn to_boxed(self, b: &mut FunctionBuilder, v: Value) -> Value {
        match self {
            Elem::Int => box_int(b, v),
            Elem::Float => box_f64(b, v),
            Elem::Boxed => v,
        }
    }
}

/// The receiver's element buffer, length and representation — its cached
/// view when one is set, resolved (and cached) otherwise — or `slow`.
fn view(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    exec_ctx: Value,
    object: u32,
    slow: Block,
) -> Result<(Value, Value, Value), String> {
    let Some(v) = ctx.views.of(object) else {
        let obj = heap::boxed_value(b, ctx, values, object)?;
        return Ok(resolve(b, ctx, exec_ctx, obj, slow));
    };
    let data = b.use_var(v.data);
    let len = b.use_var(v.len);
    let disc = b.use_var(v.disc);
    let ready = b.create_block();
    for _ in 0..3 {
        b.append_block_param(ready, types::I64);
    }
    let miss = b.create_block();
    b.ins().brif(
        data,
        ready,
        &[data.into(), len.into(), disc.into()],
        miss,
        &[],
    );

    b.switch_to_block(miss);
    let obj = heap::boxed_value(b, ctx, values, object)?;
    let (d, l, di) = resolve(b, ctx, exec_ctx, obj, slow);
    b.def_var(v.data, d);
    b.def_var(v.len, l);
    b.def_var(v.disc, di);
    b.ins().jump(ready, &[d.into(), l.into(), di.into()]);

    b.switch_to_block(ready);
    let p = b.block_params(ready);
    Ok((p[0], p[1], p[2]))
}

/// Resolve boxed receiver `obj` to its buffer, length and representation,
/// or branch to `slow` when it is not an array.
fn resolve(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    exec_ctx: Value,
    obj: Value,
    slow: Block,
) -> (Value, Value, Value) {
    let lay = &ctx.helpers.array_layout;
    let payload = cached_payload(
        b,
        exec_ctx,
        obj,
        lay,
        ctx.helpers.heap_field_offset,
        slow,
        None,
        false,
    );
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

/// Branch to `slow` unless `key < len`, continuing in a fresh block.
fn bounds(b: &mut FunctionBuilder, key: Value, len: Value, slow: Block) {
    let in_bounds = b.ins().icmp(IntCC::UnsignedLessThan, key, len);
    let hit = b.create_block();
    b.ins().brif(in_bounds, hit, &[], slow, &[]);
    b.switch_to_block(hit);
}

/// `object[index]`, in the destination's class.
pub(super) fn emit_get(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    index: u32,
    dest: Option<u32>,
) -> Result<Out, String> {
    let exec_ctx = heap::exec_ctx(ctx)?;
    let want = Elem::of(dest.map_or(SlotKind::Dynamic, |d| ctx.ssa.value_ty(d)));
    let key = load_value(b, ctx, values, index)?;

    let slow = b.create_block();
    b.set_cold_block(slow);
    let merge = b.create_block();
    b.append_block_param(merge, want.clif_ty());

    let (data, len, disc) = view(b, ctx, values, exec_ctx, object, slow)?;
    bounds(b, key, len, slow);

    let matched = b.create_block();
    let other = b.create_block();
    let is_match = b.ins().icmp_imm(IntCC::Equal, disc, want.disc());
    b.ins().brif(is_match, matched, &[], other, &[]);

    b.switch_to_block(matched);
    let scale = if want == Elem::Boxed { 4 } else { 3 };
    let off = b.ins().ishl_imm(key, scale);
    let addr = b.ins().iadd(data, off);
    let v = b.ins().load(want.clif_ty(), MemFlags::trusted(), addr, 0);
    b.ins().jump(merge, &[v.into()]);

    b.switch_to_block(other);
    if want == Elem::Boxed {
        // A boxed destination only reads a `Boxed` buffer raw.
        b.ins().jump(slow, &[]);
    } else {
        let is_boxed = b.ins().icmp_imm(IntCC::Equal, disc, 0);
        let boxed_arm = b.create_block();
        b.ins().brif(is_boxed, boxed_arm, &[], slow, &[]);
        b.switch_to_block(boxed_arm);
        let off = b.ins().ishl_imm(key, 4);
        let addr = b.ins().iadd(data, off);
        let raw = b.ins().load(types::I128, MemFlags::trusted(), addr, 0);
        let v = want.from_boxed(b, raw);
        b.ins().jump(merge, &[v.into()]);
    }

    b.switch_to_block(slow);
    let obj = heap::boxed_value(b, ctx, values, object)?;
    let (ot, op) = b.ins().isplit(obj);
    let boxed_key = box_int(b, key);
    let (kt, kp) = b.ins().isplit(boxed_key);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_array_get_fast,
        &[exec_ctx, ot, op, kt, kp],
    );
    let r = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    );
    let r = want.from_boxed(b, r);
    b.ins().jump(merge, &[r.into()]);

    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    Ok(match want {
        Elem::Int | Elem::Float => Out::Native(res),
        Elem::Boxed => Out::Boxed(res),
    })
}

/// `object[index] = value`.
pub(super) fn emit_set(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    index: u32,
    value: u32,
) -> Result<(), String> {
    let exec_ctx = heap::exec_ctx(ctx)?;
    let src = Elem::of(ctx.ssa.value_ty(value));
    let key = load_value(b, ctx, values, index)?;

    let slow = b.create_block();
    b.set_cold_block(slow);
    let merge = b.create_block();

    // A heap value stored into a boxed buffer needs the write barrier, which
    // the runtime's accessor applies: only scalars are stored inline.
    if src != Elem::Boxed {
        let raw = load_value(b, ctx, values, value)?;
        let (data, len, disc) = view(b, ctx, values, exec_ctx, object, slow)?;
        bounds(b, key, len, slow);

        let matched = b.create_block();
        let other = b.create_block();
        let is_match = b.ins().icmp_imm(IntCC::Equal, disc, src.disc());
        b.ins().brif(is_match, matched, &[], other, &[]);

        b.switch_to_block(matched);
        let off = b.ins().ishl_imm(key, 3);
        let addr = b.ins().iadd(data, off);
        b.ins().store(MemFlags::trusted(), raw, addr, 0);
        b.ins().jump(merge, &[]);

        b.switch_to_block(other);
        let is_boxed = b.ins().icmp_imm(IntCC::Equal, disc, 0);
        let boxed_arm = b.create_block();
        b.ins().brif(is_boxed, boxed_arm, &[], slow, &[]);
        b.switch_to_block(boxed_arm);
        let off = b.ins().ishl_imm(key, 4);
        let addr = b.ins().iadd(data, off);
        let boxed = src.to_boxed(b, raw);
        b.ins().store(MemFlags::trusted(), boxed, addr, 0);
        b.ins().jump(merge, &[]);
    } else {
        b.ins().jump(slow, &[]);
    }

    b.switch_to_block(slow);
    let obj = heap::boxed_value(b, ctx, values, object)?;
    let (ot, op) = b.ins().isplit(obj);
    let boxed_key = box_int(b, key);
    let (kt, kp) = b.ins().isplit(boxed_key);
    let (vt, vp) = heap::boxed_parts(b, ctx, values, value)?;
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_array_set_fast,
        &[exec_ctx, ot, op, kt, kp, vt, vp],
    );
    // The runtime's accessor may have reshaped this array or any alias of it.
    ctx.views.clear(b);
    b.ins().jump(merge, &[]);

    b.switch_to_block(merge);
    Ok(())
}
