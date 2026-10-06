use super::*;

pub(in crate::clif) fn retag_raw_return(
    b: &mut FunctionBuilder,
    raw_res: cranelift_codegen::ir::Value,
    return_kind: SlotKind,
) -> cranelift_codegen::ir::Value {
    match return_kind {
        SlotKind::Int => box_int(b, raw_res),
        SlotKind::Float => box_f64(b, raw_res),
        SlotKind::Bool => box_bool(b, raw_res),
        _ => raw_res,
    }
}

thread_local! {



    static DISABLED_HELPER_HIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) fn reset_disabled_helper_hit() {
    DISABLED_HELPER_HIT.with(|c| c.set(false));
}

pub(crate) fn disabled_helper_hit() -> bool {
    DISABLED_HELPER_HIT.with(|c| c.get())
}

#[inline]
fn note_if_disabled(helper: usize) {
    if helper == 0 {
        DISABLED_HELPER_HIT.with(|c| c.set(true));
    }
}

pub(in crate::clif) fn call_helper(
    b: &mut FunctionBuilder,
    cc: cranelift_codegen::isa::CallConv,
    helper: usize,
    args: &[cranelift_codegen::ir::Value],
) -> cranelift_codegen::ir::Value {
    note_if_disabled(helper);
    let mut sig = Signature::new(cc);
    for _ in 0..args.len() {
        sig.params.push(AbiParam::new(types::I64));
    }
    sig.returns.push(AbiParam::new(types::I64));
    let sig_ref = b.import_signature(sig);
    let ptr = b.ins().iconst(types::I64, helper as i64);
    let call = b.ins().call_indirect(sig_ref, ptr, args);
    b.inst_results(call)[0]
}

pub(in crate::clif) fn call_helper_void(
    b: &mut FunctionBuilder,
    cc: cranelift_codegen::isa::CallConv,
    helper: usize,
    args: &[cranelift_codegen::ir::Value],
) {
    note_if_disabled(helper);
    let mut sig = Signature::new(cc);
    for _ in 0..args.len() {
        sig.params.push(AbiParam::new(types::I64));
    }
    let sig_ref = b.import_signature(sig);
    let ptr = b.ins().iconst(types::I64, helper as i64);
    b.ins().call_indirect(sig_ref, ptr, args);
}

pub(in crate::clif) fn emit_array_payload(
    b: &mut FunctionBuilder,
    obj: cranelift_codegen::ir::Value,
    lay: &crate::JitArrayLayout,
    slow: cranelift_codegen::ir::Block,
) -> cranelift_codegen::ir::Value {
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let (obj_tag, obj_payload) = if b.func.dfg.value_type(obj) == types::I128 {
        b.ins().isplit(obj)
    } else {
        (b.ins().iconst(types::I64, HEAP_KIND), obj)
    };
    let tag = b.ins().band_imm_u(obj_tag, KIND_MASK);
    let is_heap = b.ins().icmp_imm_u(IntCC::Equal, tag, HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);

    let slot = obj_payload;
    let tagb = b.ins().uload8(types::I64, m, slot, lay.kind_off as i32);
    let is_arr = b.ins().icmp_imm_u(IntCC::Equal, tagb, lay.array_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_arr, ok, &[], slow, &[]);
    b.switch_to_block(ok);
    b.ins().load(types::I64, m, slot, lay.payload_off as i32)
}

pub(in crate::clif) fn array_disc(
    b: &mut FunctionBuilder,
    payload: cranelift_codegen::ir::Value,
    lay: &crate::JitArrayLayout,
) -> cranelift_codegen::ir::Value {
    b.ins().uload8(
        types::I64,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        payload,
        lay.disc_off as i32,
    )
}

pub(in crate::clif) fn unbox_int(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    if b.func.dfg.value_type(v) == types::I128 {
        let (_tag, payload) = b.ins().isplit(v);
        payload
    } else {
        v
    }
}

pub(in crate::clif) fn guard_overflow(
    b: &mut FunctionBuilder,
    cc: cranelift_codegen::isa::CallConv,
    exec_ctx: cranelift_codegen::ir::Value,
    helper: usize,
    r: cranelift_codegen::ir::Value,
    overflow: cranelift_codegen::ir::Value,
    lhs: cranelift_codegen::ir::Value,
    rhs: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let raise = b.create_block();
    let cont = b.create_block();
    b.ins().brif(overflow, raise, &[], cont, &[]);

    b.switch_to_block(raise);
    let ba = box_int(b, lhs);
    let bb = box_int(b, rhs);
    let (a_tag, a_payload) = b.ins().isplit(ba);
    let (b_tag, b_payload) = b.ins().isplit(bb);
    call_helper_void(
        b,
        cc,
        helper,
        &[exec_ctx, a_tag, a_payload, b_tag, b_payload],
    );
    b.ins().jump(cont, &[]);

    b.switch_to_block(cont);
    r
}

pub(in crate::clif) fn emit_instance_payload(
    b: &mut FunctionBuilder,
    obj: cranelift_codegen::ir::Value,
    olay: &crate::JitObjectLayout,
    alay: &crate::JitArrayLayout,
    invalid: cranelift_codegen::ir::Block,
) -> cranelift_codegen::ir::Value {
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let (obj_tag, obj_payload) = if b.func.dfg.value_type(obj) == types::I128 {
        b.ins().isplit(obj)
    } else {
        (b.ins().iconst(types::I64, HEAP_KIND), obj)
    };
    let tag = b.ins().band_imm_u(obj_tag, KIND_MASK);
    let is_heap = b.ins().icmp_imm_u(IntCC::Equal, tag, HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], invalid, &[]);
    b.switch_to_block(chk);

    let kind = b
        .ins()
        .uload8(types::I64, m, obj_payload, alay.kind_off as i32);
    let is_inst = b
        .ins()
        .icmp_imm_u(IntCC::Equal, kind, olay.instance_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_inst, ok, &[], invalid, &[]);
    b.switch_to_block(ok);
    b.ins().iadd_imm_u(
        obj_payload,
        (olay.instance_data_off + olay.instance_values_off) as i64,
    )
}

pub(in crate::clif) fn is_young(
    b: &mut FunctionBuilder,
    addr: cranelift_codegen::ir::Value,
    alay: &crate::JitArrayLayout,
) -> cranelift_codegen::ir::Value {
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let state = b.ins().uload8(types::I64, m, addr, alay.state_off as i32);
    b.ins()
        .icmp_imm_u(IntCC::Equal, state, alay.young_state as i64)
}
