









use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlagsData, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

use super::super::super::emit::{call_helper_void, HEAP_KIND, KIND_MASK};
use super::super::super::native_abi::{NativeClass, NativeShape, NATIVE_FRAMELESS};
use super::super::store::{clif_ty, drop_home_addrs, is_heap};
use super::super::{heap, load_value, stack_exit, Ctx, Out};

pub(crate) struct NativeCall<'a> {
    pub callee: Value,
    pub receiver: Value,
    pub args: &'a [u32],
    pub dest: Option<u32>,
}

pub(crate) fn emit(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    call: NativeCall<'_>,
    slow: impl FnOnce(&mut FunctionBuilder) -> Result<Value, String>,
) -> Result<Out, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: call in a frame-less body")?;
    let ectx = frame.exec_ctx;
    let lay = &ctx.helpers.call_layout;
    let m = MemFlagsData::trusted();
    let dest_kind = call
        .dest
        .map(|d| ctx.ssa.value_ty(d))
        .unwrap_or(SlotKind::Dynamic);
    let arg_kinds: Vec<SlotKind> = call.args.iter().map(|v| ctx.ssa.value_ty(*v)).collect();
    let shape = NativeShape::new(&arg_kinds, dest_kind);
    let Some(id) = shape.id() else {
        return Ok(Out::Boxed(slow(b)?));
    };

    let merge = b.create_block();
    b.append_block_param(
        merge,
        clif_ty(dest_kind).ok_or("from_ssa: unsupported call destination")?,
    );
    let slow_blk = b.create_block();
    b.set_cold_block(slow_blk);

    let (ctag, cpayload) = b.ins().isplit(call.callee);
    let kind = b.ins().band_imm_u(ctag, KIND_MASK);
    let is_heap_ref = b.ins().icmp_imm_u(IntCC::Equal, kind, HEAP_KIND);
    let walk = b.create_block();
    b.ins().brif(is_heap_ref, walk, &[], slow_blk, &[]);

    b.switch_to_block(walk);
    let slot = cpayload;
    let slot_tag = b.ins().uload8(
        types::I64,
        m,
        slot,
        ctx.helpers.array_layout.kind_off as i32,
    );
    let is_closure = b
        .ins()
        .icmp_imm_u(IntCC::Equal, slot_tag, lay.closure_tag as i64);
    let read = b.create_block();
    b.ins().brif(is_closure, read, &[], slow_blk, &[]);

    b.switch_to_block(read);
    let control = b
        .ins()
        .load(types::I64, m, slot, lay.closure_payload_off as i32);
    let closure = b.ins().iadd_imm_u(control, lay.rc_value_off as i64);
    let proto_control = b
        .ins()
        .load(types::I64, m, closure, lay.closure_proto_off as i32);
    let proto = b.ins().iadd_imm_u(proto_control, lay.rc_value_off as i64);
    let entry = b
        .ins()
        .load(types::I64, m, proto, lay.proto_native_off as i32);
    let sig = b
        .ins()
        .load(types::I64, m, proto, lay.proto_native_sig_off as i32);
    let epoch = b
        .ins()
        .load(types::I64, m, proto, lay.proto_epoch_off as i32);
    let has_entry = b.ins().icmp_imm_u(IntCC::NotEqual, entry, 0);
    let sig_core = b.ins().band_imm_u(sig, !NATIVE_FRAMELESS as i64);
    let same_sig = b.ins().icmp_imm_u(IntCC::Equal, sig_core, id as i64);
    let baked_epoch = frame.linker.current_epoch();
    let same_epoch = b.ins().icmp_imm_u(IntCC::Equal, epoch, baked_epoch as i64);
    let matched = b.ins().band(same_sig, same_epoch);
    let enter = b.ins().band(has_entry, matched);
    let entered = b.create_block();
    b.ins().brif(enter, entered, &[], slow_blk, &[]);

    b.switch_to_block(entered);
    let frameless_bit = b.ins().band_imm_u(sig, NATIVE_FRAMELESS as i64);
    let frameless = b.ins().icmp_imm_u(IntCC::NotEqual, frameless_bit, 0);
    let push = b.create_block();
    let invoke = b.create_block();
    b.ins().brif(frameless, invoke, &[], push, &[]);

    b.switch_to_block(push);
    push_frame(b, ctx, ectx, closure);
    b.ins().jump(invoke, &[]);

    b.switch_to_block(invoke);
    let (rtag, rpayload) = b.ins().isplit(call.receiver);
    let mut argv = vec![ectx, closure, rtag, rpayload];
    for (v, class) in call.args.iter().zip(&shape.params[1..]) {
        match class {
            NativeClass::Word | NativeClass::Float => argv.push(load_value(b, ctx, values, *v)?),
            NativeClass::Boxed => {
                let boxed = heap::boxed_value(b, ctx, values, *v)?;
                let (t, p) = b.ins().isplit(boxed);
                argv.push(t);
                argv.push(p);
            }
        }
    }
    let sig_ref = b.import_signature(shape.signature());
    let inst = b.ins().call_indirect(sig_ref, entry, &argv);
    let res = b.inst_results(inst).to_vec();
    let pop = b.create_block();
    let after = b.create_block();
    b.ins().brif(frameless, after, &[], pop, &[]);

    b.switch_to_block(pop);
    pop_frame(b, ctx, ectx);
    b.ins().jump(after, &[]);

    b.switch_to_block(after);
    stack_exit::publish(b, ctx.helpers, ectx);
    let direct = match shape.ret {
        NativeClass::Word | NativeClass::Float => res[0],
        NativeClass::Boxed => b.ins().iconcat(res[0], res[1]),
    };
    drop_home_addrs(ctx);
    b.ins().jump(merge, &[direct.into()]);

    b.switch_to_block(slow_blk);
    let boxed = slow(b)?;
    let fallback = if is_heap(dest_kind) {
        boxed
    } else {
        heap::unbox_dest(b, dest_kind, boxed)?
    };
    drop_home_addrs(ctx);
    b.ins().jump(merge, &[fallback.into()]);

    b.switch_to_block(merge);
    Ok(Out::Native(b.block_params(merge)[0]))
}

fn push_frame(b: &mut FunctionBuilder, ctx: &Ctx<'_>, ectx: Value, closure: Value) {
    let lay = &ctx.helpers.call_layout;
    let m = MemFlagsData::trusted();
    let len = b.ins().load(types::I64, m, ectx, lay.frames_len_off as i32);
    let cap = b.ins().load(types::I64, m, ectx, lay.frames_cap_off as i32);
    let full = b.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, len, cap);
    let deep = b.ins().icmp_imm_u(
        IntCC::UnsignedGreaterThanOrEqual,
        len,
        lay.max_call_depth as i64,
    );
    let out_of_line = b.ins().bor(full, deep);
    let inline = b.create_block();
    let grow = b.create_block();
    let done = b.create_block();
    b.set_cold_block(grow);
    b.ins().brif(out_of_line, grow, &[], inline, &[]);

    b.switch_to_block(grow);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_push_native_frame,
        &[ectx, closure],
    );
    b.ins().jump(done, &[]);

    b.switch_to_block(inline);
    let base = b.ins().load(types::I64, m, ectx, lay.frames_ptr_off as i32);
    let off = b.ins().imul_imm_u(len, lay.frame_size as i64);
    let slot = b.ins().iadd(base, off);
    let control = b.ins().iadd_imm_s(closure, -(lay.rc_value_off as i64));
    let strong = b
        .ins()
        .load(types::I64, m, control, lay.rc_strong_off as i32);
    let strong = b.ins().iadd_imm_u(strong, 1);
    b.ins().store(m, strong, control, lay.rc_strong_off as i32);
    b.ins()
        .store(m, closure, slot, lay.frame_closure_ptr_off as i32);
    b.ins().store(m, control, slot, lay.frame_owned_off as i32);
    let zero = b.ins().iconst(types::I64, 0);
    b.ins().store(m, zero, slot, lay.frame_ip_off as i32);
    b.ins().store(m, zero, slot, lay.frame_class_off as i32);
    let no_activation = b.ins().iconst(types::I64, lay.no_activation as i64);
    b.ins()
        .store(m, no_activation, slot, lay.frame_base_off as i32);
    let no_return = b.ins().iconst(types::I64, lay.no_return_reg as i64);
    b.ins()
        .istore16(m, no_return, slot, lay.frame_return_reg_off as i32);
    let len = b.ins().iadd_imm_u(len, 1);
    b.ins().store(m, len, ectx, lay.frames_len_off as i32);
    b.ins().jump(done, &[]);

    b.switch_to_block(done);
}

fn pop_frame(b: &mut FunctionBuilder, ctx: &Ctx<'_>, ectx: Value) {
    let lay = &ctx.helpers.call_layout;
    let m = MemFlagsData::trusted();
    let len = b.ins().load(types::I64, m, ectx, lay.frames_len_off as i32);
    let len = b.ins().iadd_imm_s(len, -1);
    b.ins().store(m, len, ectx, lay.frames_len_off as i32);
    let base = b.ins().load(types::I64, m, ectx, lay.frames_ptr_off as i32);
    let off = b.ins().imul_imm_u(len, lay.frame_size as i64);
    let slot = b.ins().iadd(base, off);
    let closure = b
        .ins()
        .load(types::I64, m, slot, lay.frame_closure_ptr_off as i32);
    let control = b.ins().iadd_imm_s(closure, -(lay.rc_value_off as i64));
    let strong = b
        .ins()
        .load(types::I64, m, control, lay.rc_strong_off as i32);
    let last = b.ins().icmp_imm_u(IntCC::Equal, strong, 1);
    let release = b.create_block();
    let keep = b.create_block();
    let done = b.create_block();
    b.set_cold_block(release);
    b.ins().brif(last, release, &[], keep, &[]);

    b.switch_to_block(release);
    call_helper_void(b, ctx.cc, ctx.helpers.jit_release_closure, &[closure]);
    b.ins().jump(done, &[]);

    b.switch_to_block(keep);
    let strong = b.ins().iadd_imm_s(strong, -1);
    b.ins().store(m, strong, control, lay.rc_strong_off as i32);
    b.ins().jump(done, &[]);

    b.switch_to_block(done);
}
