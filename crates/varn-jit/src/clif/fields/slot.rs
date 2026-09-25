//! Fixed-field reads by dynamic `slot`: object/record fields and enum
//! payloads (`FieldAccess::Slot`), whose storage is the object's 16-byte
//! value cells. A base cached for the loop or the block is read inline;
//! anything else asks the `get_fixed_field` helper, which knows the shape and
//! the overflow store.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;

use super::super::alloc::AllocCtx;
use super::super::emit::{
    self, call_helper_void, meta_is_float, state_meta_int, unbox_f64_coerce, unbox_int, use_boxed,
    HEAP_KIND,
};
use super::super::kinds::K;
use super::FldCtx;

/// `GetFixedField first_reg, obj, slot` by dynamic slot. When the register
/// meta proves the destination is `int`/`float`/`bool`, the fast paths load
/// only the 8-byte payload word of the cell, not the whole `VmValue`.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_get(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    actx: Option<&AllocCtx>,
    state: &[K],
    ip: usize,
    first_reg: usize,
    obj_r: usize,
    slot: usize,
) -> Result<(), String> {
    use varn_types::register_meta::SlotKind;

    // Prefer the home slot (authoritative for a nullable `Ref` receiver, which
    // the variable cannot distinguish from a heap ref).
    let obj = if let Some(actx) = actx {
        super::super::alloc::box_or_load_home(b, actx, state, obj_r)
    } else {
        use_boxed(b, c.vars, state, obj_r)?
    };

    // ── Narrow-load decision ────────────────────────────────────────────
    // When the register meta statically proves the field is a primitive,
    // load only the i64 payload (offset +8 within each 16-byte VmValue
    // slot), avoiding the i128 load + isplit + unbox chain.
    let dest_kind = c.register_meta.get(first_reg).map(|m| m.kind);
    let narrow = matches!(
        dest_kind,
        Some(SlotKind::Int) | Some(SlotKind::Float) | Some(SlotKind::Bool)
    );

    let slow = b.create_block();
    let cont = b.create_block();

    let (cont_ty, load_ty) = if narrow {
        (types::I64, types::I64)
    } else {
        (types::I128, types::I128)
    };
    b.append_block_param(cont, cont_ty);

    let slot_off = (slot * 16) as i32;
    // For narrow loads: skip the 8-byte tag word, read the payload directly.
    let load_off = if narrow { slot_off + 8 } else { slot_off };

    // ── Fast path: loop-invariant cache ─────────────────────────────────
    if let Some(cache) = c.loop_caches.object(ip, obj_r) {
        let base = b.use_var(cache.data_base);
        let ok_cache = b.ins().icmp_imm(IntCC::NotEqual, base, 0);
        let fast_blk = b.create_block();
        let unhoisted_blk = b.create_block();
        b.ins().brif(ok_cache, fast_blk, &[], unhoisted_blk, &[]);

        b.switch_to_block(fast_blk);
        let val = b.ins().load(load_ty, MemFlags::trusted(), base, load_off);
        b.ins().jump(cont, &[val.into()]);

        b.switch_to_block(unhoisted_blk);
    }

    // ── Fast path: local object-base cache ──────────────────────────────
    if let Some(&local_var) = c.local_obj_bases.get(&obj_r) {
        let local_base = b.use_var(local_var);
        let ok_local = b.ins().icmp_imm(IntCC::NotEqual, local_base, 0);
        let fast_local = b.create_block();
        let miss_local = b.create_block();
        b.ins().brif(ok_local, fast_local, &[], miss_local, &[]);

        b.switch_to_block(fast_local);
        let val = b
            .ins()
            .load(load_ty, MemFlags::trusted(), local_base, load_off);
        b.ins().jump(cont, &[val.into()]);

        b.switch_to_block(miss_local);
        let computed_base = emit::emit_object_data_base(
            b,
            c.exec_ctx,
            obj,
            &c.helpers.object_layout,
            &c.helpers.array_layout,
            c.helpers.heap_field_offset,
            slow,
        );
        b.def_var(local_var, computed_base);
        let val = b
            .ins()
            .load(load_ty, MemFlags::trusted(), computed_base, load_off);
        b.ins().jump(cont, &[val.into()]);
    } else {
        // No cached base: the helper resolves the slot (it knows the shape
        // and the overflow store).
        b.ins().jump(slow, &[]);
    }

    // ── Slow path: runtime helper ───────────────────────────────────────
    b.switch_to_block(slow);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    let (obj_tag, obj_payload) = if b.func.dfg.value_type(obj) == types::I128 {
        b.ins().isplit(obj)
    } else {
        (b.ins().iconst(types::I64, HEAP_KIND), obj)
    };
    call_helper_void(
        b,
        c.cc,
        c.helpers.get_fixed_field,
        &[c.exec_ctx, obj_tag, obj_payload, slot_v],
    );
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        c.exec_ctx,
        c.helpers.jit_native_result_offset as i32,
    );
    if narrow {
        // Helper returns a full i128 VmValue; extract only the payload.
        let (_tag, payload) = b.ins().isplit(res);
        b.ins().jump(cont, &[payload.into()]);
    } else {
        b.ins().jump(cont, &[res.into()]);
    }

    // ── Result definition ───────────────────────────────────────────────
    b.switch_to_block(cont);
    let v = b.block_params(cont)[0];

    if narrow {
        // `v` is the raw i64 payload — no unboxing needed. The destination
        // CLASS decides storage: `Bool` is `SlotClass::Dyn`, so its variable is
        // an `I128` pair and must be boxed (writing the raw payload into it is
        // a Cranelift type error).
        match dest_kind {
            Some(SlotKind::Float) => {
                let f = b.ins().bitcast(types::F64, MemFlags::new(), v);
                b.def_var(c.vars[first_reg], f);
                if let Some(actx) = actx {
                    let payload = b.ins().bitcast(types::I64, MemFlags::new(), f);
                    let tag_v =
                        b.ins()
                            .iconst(types::I64, varn_types::vm_value::KIND_FLOAT as i64);
                    let boxed = b.ins().iconcat(tag_v, payload);
                    super::super::alloc::store_boxed_home(b, actx, first_reg, boxed);
                }
            }
            Some(SlotKind::Bool) => {
                emit::def_bool_result(b, actx, c.register_meta, c.vars, first_reg, v);
            }
            _ => {
                // Int: the payload IS the native value.
                emit::def_int_result(b, actx, c.register_meta, c.vars, first_reg, v);
            }
        }
    } else if let Some(actx) = actx {
        super::super::alloc::def_result(b, actx, first_reg, v);
    } else if meta_is_float(c.register_meta, first_reg) {
        let f = unbox_f64_coerce(b, v);
        b.def_var(c.vars[first_reg], f);
    } else if state_meta_int(c.register_meta, first_reg) {
        let i = unbox_int(b, v);
        b.def_var(c.vars[first_reg], i);
    } else {
        emit::def_boxed_leaf(b, c.register_meta, c.vars, first_reg, v);
    }
    Ok(())
}
