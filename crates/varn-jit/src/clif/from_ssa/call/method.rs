use cranelift_codegen::ir::{InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::super::emit::{box_bool, box_int, call_helper};
use super::super::{heap, props, store, Ctx, Out};
use super::direct::{entry_out_slot, run_entered_or};
use super::invoke::boxed_window;
use super::native_entry::{self, NativeCall};

#[allow(clippy::too_many_arguments)]
fn emit_is_str(
    b: &mut FunctionBuilder,
    alay: &crate::JitArrayLayout,
    tag: Value,
    payload: Value,
    yes: cranelift_codegen::ir::Block,
    no: cranelift_codegen::ir::Block,
) {
    use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder};
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let k = b
        .ins()
        .band_imm_u(tag, super::super::super::emit::KIND_MASK);
    let is_sso = b
        .ins()
        .icmp_imm_u(IntCC::Equal, k, varn_types::vm_value::KIND_SSO as i64);
    let is_heap = b
        .ins()
        .icmp_imm_u(IntCC::Equal, k, super::super::super::emit::HEAP_KIND);
    let walk = b.create_block();
    let not_sso = b.create_block();
    b.ins().brif(is_sso, yes, &[], not_sso, &[]);
    b.switch_to_block(not_sso);
    b.ins().brif(is_heap, walk, &[], no, &[]);
    b.switch_to_block(walk);
    let tagb = b.ins().uload8(types::I64, m, payload, alay.kind_off as i32);
    let is_str = b.ins().icmp_imm_u(IntCC::Equal, tagb, alay.str_tag as i64);
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
    dest: Option<u32>,
) -> Result<Value, String> {
    use cranelift_codegen::ir::condcodes::IntCC;
    use cranelift_codegen::ir::types;
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
    let out = entry_out_slot(b);

    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let olay = &ctx.helpers.object_layout;
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
            &ctx.helpers.array_layout,
            rt0,
            rp0,
            arg_check,
            inst_entry,
        );
        b.switch_to_block(arg_check);
        emit_is_str(b, &ctx.helpers.array_layout, at, ap, str_go, inst_entry);
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
    let kind = b.ins().band_imm_u(rt, super::super::super::emit::KIND_MASK);
    let is_heap = b
        .ins()
        .icmp_imm_u(IntCC::Equal, kind, super::super::super::emit::HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);
    let slot_addr = rp;
    let tagb = b.ins().uload8(
        types::I64,
        m,
        slot_addr,
        ctx.helpers.array_layout.kind_off as i32,
    );
    let is_inst = b
        .ins()
        .icmp_imm_u(IntCC::Equal, tagb, olay.instance_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_inst, ok, &[], slow, &[]);
    b.switch_to_block(ok);
    let cid32 = b.ins().load(
        types::I32,
        m,
        slot_addr,
        (olay.instance_data_off + olay.instance_class_id_off) as i32,
    );
    let cid = b.ins().uextend(types::I64, cid32);

    let ic_base = b.ins().load(
        types::I64,
        m,
        frame.closure,
        ctx.helpers.closure_ic_entries_offset as i32,
    );
    let slot_base = b.ins().iadd_imm_u(
        ic_base,
        i64::from(cs) * (ctx.helpers.poly_ic_slot_size as i64),
    );
    let resolved = b.create_block();
    for _ in 0..5 {
        b.append_block_param(resolved, types::I64);
    }
    let entry_size = std::mem::size_of::<varn_types::chunk::CacheEntry>() as i64;
    let mut next = b.create_block();
    b.ins().jump(next, &[]);
    for i in 0..8 {
        b.switch_to_block(next);
        next = b.create_block();
        let hit = b.create_block();
        let entry = b.ins().iadd_imm_u(slot_base, i * entry_size);
        let id32 = b.ins().load(types::I32, m, entry, 0);
        let id = b.ins().uextend(types::I64, id32);
        let kc = b.ins().uload8(types::I64, m, entry, 6);
        let id_eq = b.ins().icmp(IntCC::Equal, id, cid);
        let is_native = b.ins().icmp_imm_u(
            IntCC::Equal,
            kc,
            varn_types::chunk::ICKind::NATIVE_VTABLE_METHOD as i64,
        );
        let is_vm = b.ins().icmp_imm_u(
            IntCC::Equal,
            kc,
            varn_types::chunk::ICKind::VM_VTABLE_METHOD as i64,
        );
        let is_vtable = b.ins().bor(is_native, is_vm);
        let matched = b.ins().band(id_eq, is_vtable);
        b.ins().brif(matched, hit, &[], next, &[]);

        b.switch_to_block(hit);
        let control = b.ins().load(types::I64, m, entry, 8);
        let has_class = b.ins().icmp_imm_u(IntCC::NotEqual, control, 0);
        let go = b.create_block();
        b.ins().brif(has_class, go, &[], next, &[]);
        b.switch_to_block(go);
        let class = b
            .ins()
            .iadd_imm_u(control, ctx.helpers.call_layout.rc_value_off as i64);
        let slot16 = b.ins().uload16(types::I64, m, entry, 4);
        let ver8 = b.ins().uload8(types::I64, m, entry, 7);
        b.ins().jump(
            resolved,
            &[
                class.into(),
                id.into(),
                slot16.into(),
                kc.into(),
                ver8.into(),
            ],
        );
    }
    b.switch_to_block(next);
    b.ins().jump(slow, &[]);

    b.switch_to_block(resolved);
    let p = b.block_params(resolved).to_vec();
    let (class, id, slot16, kc, ver8) = (p[0], p[1], p[2], p[3], p[4]);
    let cached = |b: &mut FunctionBuilder| -> Value {
        let entry = call_helper(
            b,
            ctx.cc,
            ctx.helpers.jit_call_method_cached_window,
            &[
                ectx, class, id, slot16, kc, ver8, name_v, cs_v, window, total, out,
            ],
        );
        store::drop_home_addrs(ctx);
        run_entered_or(b, ctx, entry, out)
    };
    let lay = &ctx.helpers.call_layout;
    let is_vm = b.ins().icmp_imm_u(
        IntCC::Equal,
        kc,
        varn_types::chunk::ICKind::VM_VTABLE_METHOD as i64,
    );
    let ver_now = b
        .ins()
        .uload8(types::I64, m, class, lay.class_vtable_version_off as i32);
    let same_ver = b.ins().icmp(IntCC::Equal, ver_now, ver8);
    let vt_len = b
        .ins()
        .load(types::I64, m, class, lay.class_vtable_len_off as i32);
    let in_range = b.ins().icmp(IntCC::UnsignedLessThan, slot16, vt_len);
    let current = b.ins().band(same_ver, in_range);
    let direct = b.ins().band(is_vm, current);
    let native_blk = b.create_block();
    let cached_blk = b.create_block();
    b.ins().brif(direct, native_blk, &[], cached_blk, &[]);

    b.switch_to_block(native_blk);
    let vt_ptr = b
        .ins()
        .load(types::I64, m, class, lay.class_vtable_ptr_off as i32);
    let off = b.ins().ishl_imm_u(slot16, 4);
    let at = b.ins().iadd(vt_ptr, off);
    let method = b.ins().load(types::I128, m, at, 0);
    let call = NativeCall {
        callee: method,
        receiver,
        args,
        dest,
    };
    let landed = native_entry::emit(b, ctx, values, call, |b| Ok(cached(b)))?;
    let dest_kind = dest
        .map(|d| ctx.ssa.value_ty(d))
        .unwrap_or(varn_types::register_meta::SlotKind::Dynamic);
    let boxed = match landed {
        Out::Native(v) => heap::box_native(b, dest_kind, v),
        Out::Boxed(v) => v,
    };
    b.ins().jump(merge, &[boxed.into()]);

    b.switch_to_block(cached_blk);
    let boxed = cached(b);
    b.ins().jump(merge, &[boxed.into()]);

    b.switch_to_block(slow);
    let entry = call_helper(
        b,
        ctx.cc,
        ctx.helpers.jit_call_method_window,
        &[frame.exec_ctx, name_v, cs_v, window, total, out],
    );
    store::drop_home_addrs(ctx);
    let res = run_entered_or(b, ctx, entry, out);
    b.ins().jump(merge, &[res.into()]);

    b.switch_to_block(merge);
    Ok(b.block_params(merge)[0])
}
