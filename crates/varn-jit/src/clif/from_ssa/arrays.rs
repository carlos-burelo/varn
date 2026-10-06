





















use cranelift_codegen::ir::{condcodes::IntCC, types, Block, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

use super::super::emit::{
    array_disc, box_f64, box_int, call_helper_void, emit_array_payload, unbox_f64_coerce, unbox_int,
};
use super::{heap, load_value, Ctx, Out};



#[derive(Clone, Copy, PartialEq)]
enum Elem {
    Int,
    Float,
    
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

    fn unbox_elem(self, b: &mut FunctionBuilder, v: Value) -> Value {
        match self {
            Elem::Int => unbox_int(b, v),
            Elem::Float => unbox_f64_coerce(b, v),
            Elem::Boxed => v,
        }
    }

    fn box_elem(self, b: &mut FunctionBuilder, v: Value) -> Value {
        match self {
            Elem::Int => box_int(b, v),
            Elem::Float => box_f64(b, v),
            Elem::Boxed => v,
        }
    }
}



fn view(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    slow: Block,
) -> Result<(Value, Value, Value), String> {
    let Some(v) = ctx.views.of(object) else {
        let obj = heap::boxed_value(b, ctx, values, object)?;
        return Ok(resolve(b, ctx, obj, slow));
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
    let (d, l, di) = resolve(b, ctx, obj, slow);
    b.def_var(v.data, d);
    b.def_var(v.len, l);
    b.def_var(v.disc, di);
    
    
    super::store::drop_home_addrs(ctx);
    b.ins().jump(ready, &[d.into(), l.into(), di.into()]);

    b.switch_to_block(ready);
    let p = b.block_params(ready);
    Ok((p[0], p[1], p[2]))
}



fn resolve(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    obj: Value,
    slow: Block,
) -> (Value, Value, Value) {
    let lay = &ctx.helpers.array_layout;
    let payload = emit_array_payload(b, obj, lay, slow);
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let data = b
        .ins()
        .load(types::I64, m, payload, lay.elems_ptr_off as i32);
    let len = b
        .ins()
        .load(types::I64, m, payload, lay.elems_len_off as i32);
    let disc = array_disc(b, payload, lay);
    (data, len, disc)
}


fn bounds(b: &mut FunctionBuilder, key: Value, len: Value, slow: Block) {
    let in_bounds = b.ins().icmp(IntCC::UnsignedLessThan, key, len);
    let hit = b.create_block();
    b.ins().brif(in_bounds, hit, &[], slow, &[]);
    b.switch_to_block(hit);
}


pub(super) fn emit_get(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    index: u32,
    dest: Option<u32>,
) -> Result<Out, String> {
    let exec_ctx = heap::exec_ctx(ctx);
    let want = Elem::of(dest.map_or(SlotKind::Dynamic, |d| ctx.ssa.value_ty(d)));
    let key = load_value(b, ctx, values, index)?;

    let slow = b.create_block();
    b.set_cold_block(slow);
    let merge = b.create_block();
    b.append_block_param(merge, want.clif_ty());

    let (data, len, disc) = view(b, ctx, values, object, slow)?;
    bounds(b, key, len, slow);

    let matched = b.create_block();
    let other = b.create_block();
    let is_match = b.ins().icmp_imm_u(IntCC::Equal, disc, want.disc());
    b.ins().brif(is_match, matched, &[], other, &[]);

    b.switch_to_block(matched);
    let scale = if want == Elem::Boxed { 4 } else { 3 };
    let off = b.ins().ishl_imm_u(key, scale);
    let addr = b.ins().iadd(data, off);
    let v = b.ins().load(
        want.clif_ty(),
        cranelift_codegen::ir::MemFlagsData::trusted(),
        addr,
        0,
    );
    b.ins().jump(merge, &[v.into()]);

    b.switch_to_block(other);
    if want == Elem::Boxed {
        
        b.ins().jump(slow, &[]);
    } else {
        let is_boxed = b.ins().icmp_imm_u(IntCC::Equal, disc, 0);
        let boxed_arm = b.create_block();
        b.ins().brif(is_boxed, boxed_arm, &[], slow, &[]);
        b.switch_to_block(boxed_arm);
        let off = b.ins().ishl_imm_u(key, 4);
        let addr = b.ins().iadd(data, off);
        let raw = b.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            addr,
            0,
        );
        let v = want.unbox_elem(b, raw);
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
        cranelift_codegen::ir::MemFlagsData::trusted(),
        exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    );
    let r = want.unbox_elem(b, r);
    
    
    super::store::drop_home_addrs(ctx);
    b.ins().jump(merge, &[r.into()]);

    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    Ok(match want {
        Elem::Int | Elem::Float => Out::Native(res),
        Elem::Boxed => Out::Boxed(res),
    })
}


pub(super) fn emit_set(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    index: u32,
    value: u32,
) -> Result<(), String> {
    let exec_ctx = heap::exec_ctx(ctx);
    let src = Elem::of(ctx.ssa.value_ty(value));
    let key = load_value(b, ctx, values, index)?;

    let slow = b.create_block();
    b.set_cold_block(slow);
    let merge = b.create_block();

    
    
    if src != Elem::Boxed {
        let raw = load_value(b, ctx, values, value)?;
        let (data, len, disc) = view(b, ctx, values, object, slow)?;
        bounds(b, key, len, slow);

        let matched = b.create_block();
        let other = b.create_block();
        let is_match = b.ins().icmp_imm_u(IntCC::Equal, disc, src.disc());
        b.ins().brif(is_match, matched, &[], other, &[]);

        b.switch_to_block(matched);
        let off = b.ins().ishl_imm_u(key, 3);
        let addr = b.ins().iadd(data, off);
        b.ins()
            .store(cranelift_codegen::ir::MemFlagsData::trusted(), raw, addr, 0);
        b.ins().jump(merge, &[]);

        b.switch_to_block(other);
        let is_boxed = b.ins().icmp_imm_u(IntCC::Equal, disc, 0);
        let boxed_arm = b.create_block();
        b.ins().brif(is_boxed, boxed_arm, &[], slow, &[]);
        b.switch_to_block(boxed_arm);
        let off = b.ins().ishl_imm_u(key, 4);
        let addr = b.ins().iadd(data, off);
        let boxed = src.box_elem(b, raw);
        b.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            boxed,
            addr,
            0,
        );
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
    
    ctx.views.clear(b);
    
    
    super::store::drop_home_addrs(ctx);
    b.ins().jump(merge, &[]);

    b.switch_to_block(merge);
    Ok(())
}

pub(super) fn prefill(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    objects: &[u32],
) -> Result<Block, String> {
    let done = b.create_block();
    let mut slows = Vec::with_capacity(objects.len());
    for object in objects {
        let slow = b.create_block();
        b.set_cold_block(slow);
        slows.push(slow);
        let _ = view(b, ctx, values, *object, slow)?;
    }
    b.ins().jump(done, &[]);
    for slow in slows {
        b.switch_to_block(slow);
        b.ins().jump(done, &[]);
    }
    b.switch_to_block(done);
    Ok(done)
}
