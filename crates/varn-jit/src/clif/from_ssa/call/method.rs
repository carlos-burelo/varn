use cranelift_codegen::ir::{InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::super::emit::{box_bool, box_int, call_helper, call_helper_void};
use super::super::{heap, props, store, Ctx};
use super::invoke::boxed_window;

#[allow(clippy::too_many_arguments)]
fn emit_is_str(
    b: &mut FunctionBuilder,
    ectx: Value,
    alay: &crate::JitArrayLayout,
    heap_off: usize,
    str_tag: usize,
    tag: Value,
    payload: Value,
    yes: cranelift_codegen::ir::Block,
    no: cranelift_codegen::ir::Block,
) {
    use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
    let m = MemFlags::trusted();
    let k = b.ins().band_imm(tag, super::super::super::emit::KIND_MASK);
    let is_sso = b
        .ins()
        .icmp_imm(IntCC::Equal, k, varn_types::vm_value::KIND_SSO as i64);
    let is_heap = b
        .ins()
        .icmp_imm(IntCC::Equal, k, super::super::super::emit::HEAP_KIND);
    let walk = b.create_block();
    let not_sso = b.create_block();
    b.ins().brif(is_sso, yes, &[], not_sso, &[]);
    b.switch_to_block(not_sso);
    b.ins().brif(is_heap, walk, &[], no, &[]);
    b.switch_to_block(walk);
    let raw = b.ins().band_imm(payload, 0xFFFF_FFFF);
    let rc = b.ins().load(types::I64, m, ectx, heap_off as i32);
    let old_bit = b.ins().band_imm(raw, 0x8000_0000);
    let base_old = b.ins().load(
        types::I64,
        m,
        rc,
        (alay.slots_vec_off + alay.slots_ptr_off) as i32,
    );
    let base_nur = b.ins().load(
        types::I64,
        m,
        rc,
        (alay.nursery_slots_vec_off + alay.slots_ptr_off) as i32,
    );
    let idx_old = b.ins().band_imm(raw, 0x7FFF_FFFF);
    let base = b.ins().select(old_bit, base_old, base_nur);
    let idx = b.ins().select(old_bit, idx_old, raw);
    let byte_off = b.ins().imul_imm(idx, alay.slot_size as i64);
    let slot_addr = b.ins().iadd(base, byte_off);
    let tagb = b.ins().uload8(types::I64, m, slot_addr, 0);
    let is_str = b.ins().icmp_imm(IntCC::Equal, tagb, str_tag as i64);
    b.ins().brif(is_str, yes, &[], no, &[]);
}

pub(crate) fn emit_method_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    recv: u32,
    name: &str,
    args: &[u32],
    cs: u16,
) -> Result<Value, String> {
    use cranelift_codegen::ir::condcodes::IntCC;
    use cranelift_codegen::ir::{types, MemFlags};
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: method call without a frame")?;
    let receiver = heap::boxed_value(b, ctx, values, recv)?;
    let window = boxed_window(b, ctx, values, receiver, args)?;
    let name_v = b
        .ins()
        .iconst(types::I64, props::str_idx(ctx, name)? as i64);
    let cs_v = b.ins().iconst(types::I64, i64::from(cs));
    let total = b.ins().iconst(types::I64, (args.len() + 1) as i64);

    let slow = b.create_block();
    b.set_cold_block(slow);
    let merge = b.create_block();
    b.append_block_param(merge, types::I128);

    let m = MemFlags::trusted();
    let olay = &ctx.helpers.object_layout;
    let alay = &ctx.helpers.array_layout;
    let heap_off = ctx.helpers.heap_field_offset;
    let ectx = frame.exec_ctx;

    if args.len() == 1
        && (name == "startsWith" || name == "endsWith" || name == "indexOf" || name == "includes")
    {
        let arg0 = b.ins().load(types::I128, m, window, 16);
        let (at, ap) = b.ins().isplit(arg0);
        let (rt0, rp0) = b.ins().isplit(receiver);
        let inst_entry = b.create_block();
        let arg_check = b.create_block();
        let str_go = b.create_block();
        emit_is_str(
            b,
            ectx,
            alay,
            heap_off,
            ctx.helpers.str_layout.str_tag,
            rt0,
            rp0,
            arg_check,
            inst_entry,
        );
        b.switch_to_block(arg_check);
        emit_is_str(
            b,
            ectx,
            alay,
            heap_off,
            ctx.helpers.str_layout.str_tag,
            at,
            ap,
            str_go,
            inst_entry,
        );
        b.switch_to_block(str_go);
        let helper = if name == "startsWith" {
            ctx.helpers.str_starts_with
        } else if name == "endsWith" {
            ctx.helpers.str_ends_with
        } else if name == "includes" {
            ctx.helpers.str_includes
        } else {
            ctx.helpers.str_index_of
        };
        let r = call_helper(b, ctx.cc, helper, &[ectx, rt0, rp0, at, ap]);
        let boxed = if name == "indexOf" {
            box_int(b, r)
        } else {
            box_bool(b, r)
        };
        b.ins().jump(merge, &[boxed.into()]);
        b.switch_to_block(inst_entry);
    }

    let (rt, rp) = b.ins().isplit(receiver);
    let kind = b.ins().band_imm(rt, super::super::super::emit::KIND_MASK);
    let is_heap = b
        .ins()
        .icmp_imm(IntCC::Equal, kind, super::super::super::emit::HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);
    let raw = b.ins().band_imm(rp, 0xFFFF_FFFF);
    let rc = b.ins().load(types::I64, m, ectx, heap_off as i32);
    let old_bit = b.ins().band_imm(raw, 0x8000_0000);
    let base_old = b.ins().load(
        types::I64,
        m,
        rc,
        (alay.slots_vec_off + alay.slots_ptr_off) as i32,
    );
    let base_nur = b.ins().load(
        types::I64,
        m,
        rc,
        (alay.nursery_slots_vec_off + alay.slots_ptr_off) as i32,
    );
    let idx_old = b.ins().band_imm(raw, 0x7FFF_FFFF);
    let base = b.ins().select(old_bit, base_old, base_nur);
    let idx = b.ins().select(old_bit, idx_old, raw);
    let byte_off = b.ins().imul_imm(idx, alay.slot_size as i64);
    let slot_addr = b.ins().iadd(base, byte_off);
    let tagb = b.ins().uload8(types::I64, m, slot_addr, 0);
    let is_inst = b
        .ins()
        .icmp_imm(IntCC::Equal, tagb, olay.instance_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_inst, ok, &[], slow, &[]);
    b.switch_to_block(ok);
    let data_ptr = b
        .ins()
        .load(types::I64, m, slot_addr, olay.instance_payload_off as i32);
    let cid32 = b
        .ins()
        .load(types::I32, m, data_ptr, olay.instance_class_id_off as i32);
    let cid = b.ins().uextend(types::I64, cid32);

    let ic_base = b.ins().load(
        types::I64,
        m,
        frame.closure,
        ctx.helpers.closure_ic_entries_offset as i32,
    );
    let slot_base = b.ins().iadd_imm(
        ic_base,
        i64::from(cs) * (ctx.helpers.poly_ic_slot_size as i64),
    );
    let mut next = b.create_block();
    b.ins().jump(next, &[]);
    for i in 0..8 {
        b.switch_to_block(next);
        next = b.create_block();
        let hit = b.create_block();
        let entry = b.ins().iadd_imm(slot_base, (i * 8) as i64);
        let id32 = b.ins().load(types::I32, m, entry, 0);
        let id = b.ins().uextend(types::I64, id32);
        let kc = b.ins().uload8(types::I64, m, entry, 6);
        let id_eq = b.ins().icmp(IntCC::Equal, id, cid);
        let is_native = b.ins().icmp_imm(
            IntCC::Equal,
            kc,
            varn_types::chunk::ICKind::NATIVE_VTABLE_METHOD as i64,
        );
        let is_vm = b.ins().icmp_imm(
            IntCC::Equal,
            kc,
            varn_types::chunk::ICKind::VM_VTABLE_METHOD as i64,
        );
        let is_vtable = b.ins().bor(is_native, is_vm);
        let matched = b.ins().band(id_eq, is_vtable);
        b.ins().brif(matched, hit, &[], next, &[]);

        b.switch_to_block(hit);
        let classp = b.ins().load(types::I64, m, entry, 8);
        let has_class = b.ins().icmp_imm(IntCC::NotEqual, classp, 0);
        let go = b.create_block();
        b.ins().brif(has_class, go, &[], next, &[]);
        b.switch_to_block(go);
        let slot16 = b.ins().uload16(types::I64, m, entry, 4);
        let ver8 = b.ins().uload8(types::I64, m, entry, 7);
        call_helper_void(
            b,
            ctx.cc,
            ctx.helpers.jit_call_method_cached_window,
            &[
                ectx, classp, id, slot16, kc, ver8, name_v, cs_v, window, total,
            ],
        );
        store::drop_home_addrs(ctx);
        let hit_res = b.ins().load(
            types::I128,
            MemFlags::trusted(),
            ectx,
            ctx.helpers.jit_native_result_offset as i32,
        );
        store::drop_home_addrs(ctx);
        b.ins().jump(merge, &[hit_res.into()]);
    }
    b.switch_to_block(next);
    b.ins().jump(slow, &[]);

    b.switch_to_block(slow);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_call_method_window,
        &[frame.exec_ctx, name_v, cs_v, window, total],
    );
    store::drop_home_addrs(ctx);
    let slow_res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    );
    store::drop_home_addrs(ctx);
    b.ins().jump(merge, &[slow_res.into()]);

    b.switch_to_block(merge);
    Ok(b.block_params(merge)[0])
}
