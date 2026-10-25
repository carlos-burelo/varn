use cranelift_codegen::ir::{condcodes::IntCC, types, Block, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_core::layout::{ScalarRepr, COMPACT_REF_NULL};
use varn_types::chunk::{CacheEntry, ICKind};
use varn_types::vm_value::{KIND_HEAP, KIND_NULL};

use super::super::super::emit::{box_bool, box_f64, box_int, KIND_MASK};
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
    let reprs = [
        ScalarRepr::I64,
        ScalarRepr::F64,
        ScalarRepr::Bool,
        ScalarRepr::Ref,
        ScalarRepr::Boxed,
    ];
    let mut next = b.create_block();
    b.ins().jump(next, &[]);
    for repr in reprs {
        b.switch_to_block(next);
        next = b.create_block();
        let read = b.create_block();
        let is_repr = b
            .ins()
            .icmp_imm_u(IntCC::Equal, access, i64::from(repr.ic_code()));
        b.ins().brif(is_repr, read, &[], next, &[]);
        b.switch_to_block(read);
        let boxed = match repr {
            ScalarRepr::I64 => {
                let v = b.ins().load(types::I64, m, addr, 0);
                box_int(b, v)
            }
            ScalarRepr::F64 => {
                let f = b.ins().load(types::F64, m, addr, 0);
                box_f64(b, f)
            }
            ScalarRepr::Bool => {
                let b8 = b.ins().load(types::I8, m, addr, 0);
                let v = b.ins().uextend(types::I64, b8);
                box_bool(b, v)
            }
            ScalarRepr::Ref => {
                let raw = b.ins().load(types::I64, m, addr, 0);
                let is_null = b
                    .ins()
                    .icmp_imm_u(IntCC::Equal, raw, COMPACT_REF_NULL as i64);
                let null_tag = b.ins().iconst(types::I64, KIND_NULL as i64);
                let heap_tag = b.ins().iconst(types::I64, KIND_HEAP as i64);
                let zero = b.ins().iconst(types::I64, 0);
                let tag = b.ins().select(is_null, null_tag, heap_tag);
                let payload = b.ins().select(is_null, zero, raw);
                b.ins().iconcat(tag, payload)
            }
            ScalarRepr::Boxed => b.ins().load(types::I128, m, addr, 0),
        };
        b.ins().jump(merge, &[boxed.into()]);
    }
    b.switch_to_block(next);
    b.ins().jump(slow, &[]);
}

pub(super) fn emit_instance_store(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    object_payload: Value,
    cell_kind: Value,
    closure: Value,
    cs: u16,
    value: Value,
    young: Value,
    cont: Block,
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
    let (value_tag, value_payload) = b.ins().isplit(value);
    let value_kind = b.ins().band_imm_u(value_tag, KIND_MASK);

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
        emit_scalar_write(
            b,
            access,
            addr,
            value,
            value_kind,
            value_payload,
            young,
            cont,
            slow,
        );
    }
    b.switch_to_block(next);
    b.ins().jump(slow, &[]);
}

fn emit_scalar_write(
    b: &mut FunctionBuilder,
    access: Value,
    addr: Value,
    value: Value,
    value_kind: Value,
    value_payload: Value,
    young: Value,
    cont: Block,
    slow: Block,
) {
    use varn_types::vm_value::{KIND_BOOL, KIND_FLOAT, KIND_INT};
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let reprs = [
        ScalarRepr::I64,
        ScalarRepr::F64,
        ScalarRepr::Bool,
        ScalarRepr::Ref,
        ScalarRepr::Boxed,
    ];
    let mut next = b.create_block();
    b.ins().jump(next, &[]);
    for repr in reprs {
        b.switch_to_block(next);
        next = b.create_block();
        let write = b.create_block();
        let is_repr = b
            .ins()
            .icmp_imm_u(IntCC::Equal, access, i64::from(repr.ic_code()));
        b.ins().brif(is_repr, write, &[], next, &[]);
        b.switch_to_block(write);
        match repr {
            ScalarRepr::I64 | ScalarRepr::F64 | ScalarRepr::Bool => {
                let want = match repr {
                    ScalarRepr::I64 => KIND_INT,
                    ScalarRepr::F64 => KIND_FLOAT,
                    ScalarRepr::Bool => KIND_BOOL,
                    ScalarRepr::Ref | ScalarRepr::Boxed => {
                        unreachable!("the enclosing arm only admits scalar reprs")
                    }
                };
                let kind_ok = b.ins().icmp_imm_u(IntCC::Equal, value_kind, want as i64);
                let store = b.create_block();
                b.ins().brif(kind_ok, store, &[], slow, &[]);
                b.switch_to_block(store);
                if repr == ScalarRepr::Bool {
                    let byte = b.ins().ireduce(types::I8, value_payload);
                    b.ins().store(m, byte, addr, 0);
                } else {
                    b.ins().store(m, value_payload, addr, 0);
                }
                b.ins().jump(cont, &[]);
            }
            ScalarRepr::Ref => {
                let heap_or_null = b.create_block();
                let is_heap = b
                    .ins()
                    .icmp_imm_u(IntCC::Equal, value_kind, KIND_HEAP as i64);
                let is_null = b
                    .ins()
                    .icmp_imm_u(IntCC::Equal, value_kind, KIND_NULL as i64);
                let ok = b.ins().bor(is_heap, is_null);
                let young_ok = b.ins().bor(young, is_null);
                let both = b.ins().band(ok, young_ok);
                b.ins().brif(both, heap_or_null, &[], slow, &[]);
                b.switch_to_block(heap_or_null);
                let zero = b.ins().iconst(types::I64, COMPACT_REF_NULL as i64);
                let raw = b.ins().select(is_null, zero, value_payload);
                b.ins().store(m, raw, addr, 0);
                b.ins().jump(cont, &[]);
            }
            ScalarRepr::Boxed => {
                let store = b.create_block();
                b.ins().brif(young, store, &[], slow, &[]);
                b.switch_to_block(store);
                b.ins().store(m, value, addr, 0);
                b.ins().jump(cont, &[]);
            }
        }
    }
    b.switch_to_block(next);
    b.ins().jump(slow, &[]);
}
