use cranelift_codegen::ir::{condcodes::IntCC, types, Block, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_core::{FieldAccess, RuntimeKind};
use varn_types::chunk::{CacheEntry, ICKind};

use super::super::super::emit::{box_bool, box_f64, box_int};
use super::super::Ctx;

pub(super) fn emit_instance_hit(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    object_payload: Value,
    cell_kind: Value,
    closure: Value,
    cs: u16,
    merge: Block,
    slow: Block,
) {
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let is_inst = b.ins().icmp_imm_u(
        IntCC::Equal,
        cell_kind,
        varn_types::cell::CELL_KIND_INSTANCE as i64,
    );
    let inst = b.create_block();
    b.ins().brif(is_inst, inst, &[], slow, &[]);
    b.switch_to_block(inst);

    let class_id32 = b.ins().load(
        types::I32,
        m,
        object_payload,
        (varn_types::cell::INST_CELL_DATA_OFF + varn_types::INST_CLASS_ID_OFF) as i32,
    );
    let class_id = b.ins().uextend(types::I64, class_id32);
    let data_base = b.ins().iadd_imm_u(
        object_payload,
        (varn_types::cell::INST_CELL_DATA_OFF + varn_types::INST_PAYLOAD_OFF) as i64,
    );
    let ic_base = b.ins().load(
        types::I64,
        m,
        closure,
        ctx.helpers.closure_ic_entries_offset as i32,
    );
    let slot_base = b.ins().iadd_imm_u(
        ic_base,
        i64::from(cs) * (ctx.helpers.poly_ic_slot_size as i64),
    );

    let mut next = b.create_block();
    b.ins().jump(next, &[]);
    for i in 0..8 {
        b.switch_to_block(next);
        next = b.create_block();
        let hit = b.create_block();
        let entry = b
            .ins()
            .iadd_imm_u(slot_base, (i * std::mem::size_of::<CacheEntry>()) as i64);
        let id32 = b.ins().load(types::I32, m, entry, 0);
        let id = b.ins().uextend(types::I64, id32);
        let kind = b.ins().uload8(types::I64, m, entry, 6);
        let id_eq = b.ins().icmp(IntCC::Equal, id, class_id);
        let kind_ok = b
            .ins()
            .icmp_imm_u(IntCC::Equal, kind, ICKind::INSTANCE_FIELD as i64);
        let matched = b.ins().band(id_eq, kind_ok);
        b.ins().brif(matched, hit, &[], next, &[]);

        b.switch_to_block(hit);
        let offset32 = b.ins().load(
            types::I32,
            m,
            entry,
            std::mem::offset_of!(CacheEntry, offset) as i32,
        );
        let offset = b.ins().uextend(types::I64, offset32);
        let access = b.ins().uload8(
            types::I64,
            m,
            entry,
            std::mem::offset_of!(CacheEntry, access) as i32,
        );
        let addr = b.ins().iadd(data_base, offset);
        emit_scalar_read(b, access, addr, merge, slow);
    }
    b.switch_to_block(next);
    b.ins().jump(slow, &[]);
}

fn emit_scalar_read(
    b: &mut FunctionBuilder,
    access: Value,
    addr: Value,
    merge: Block,
    slow: Block,
) {
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let tag = |kind: RuntimeKind| i64::from(FieldAccess::Compact(Some(kind)).encode());

    let int_blk = b.create_block();
    let not_int = b.create_block();
    let is_int = b
        .ins()
        .icmp_imm_u(IntCC::Equal, access, tag(RuntimeKind::Int));
    b.ins().brif(is_int, int_blk, &[], not_int, &[]);
    b.switch_to_block(int_blk);
    let v = b.ins().load(types::I64, m, addr, 0);
    let boxed = box_int(b, v);
    b.ins().jump(merge, &[boxed.into()]);

    b.switch_to_block(not_int);
    let float_blk = b.create_block();
    let not_float = b.create_block();
    let is_float = b
        .ins()
        .icmp_imm_u(IntCC::Equal, access, tag(RuntimeKind::Float));
    b.ins().brif(is_float, float_blk, &[], not_float, &[]);
    b.switch_to_block(float_blk);
    let f = b.ins().load(types::F64, m, addr, 0);
    let boxed = box_f64(b, f);
    b.ins().jump(merge, &[boxed.into()]);

    b.switch_to_block(not_float);
    let bool_blk = b.create_block();
    let is_bool = b
        .ins()
        .icmp_imm_u(IntCC::Equal, access, tag(RuntimeKind::Bool));
    b.ins().brif(is_bool, bool_blk, &[], slow, &[]);
    b.switch_to_block(bool_blk);
    let b8 = b.ins().load(types::I8, m, addr, 0);
    let v = b.ins().uextend(types::I64, b8);
    let boxed = box_bool(b, v);
    b.ins().jump(merge, &[boxed.into()]);
}
