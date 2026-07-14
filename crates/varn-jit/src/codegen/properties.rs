use varn_core::OpCode;
use crate::assembler::{Reg, Cond};
use crate::regalloc::{
    emit_flush_all, emit_load, emit_reload_all, emit_reload_all_except, emit_store,
};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_properties(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::GetProperty => emit_get_property(ctx, first_reg),
        OpCode::SetProperty => emit_set_property(ctx, first_reg),
        _ => unreachable!("emit_properties called with {:?}", op),
    }
    Ok(())
}

fn emit_get_property(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let obj_reg = (w1 >> 8) as usize;
    let cs_idx = (w1 & 0xFF) as usize;
    let name_idx = code[*ip] as usize;
    *ip += 1;

    let lay = helpers.array_layout;
    let obj_lay = helpers.object_layout;
    let heap_off = helpers.heap_field_offset;

    let mut done_ic: Vec<usize> = Vec::new();
    {
        // 0. Check cs_idx < closure.ic_cache_len()
        asm.mov_reg_mem(Reg::R10, Reg::Rsp, 8);     // Load closure from stack
        asm.mov_reg_mem(Reg::R10, Reg::R10, 40);    // RcBox of ic_cache
        asm.mov_reg_mem(Reg::R11, Reg::R10, 40);    // ic_cache_len
        asm.cmp_reg_imm32(Reg::R11, cs_idx as i32);
        let p_oob = asm.jmp_cond(Cond::BelowEqual);

        emit_load(asm, Reg::Rax, obj_reg, regmap);

        // 1. Check if obj is a heap pointer
        let heap_mask =
            varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::MASK_TAG;
        let heap_expect =
            varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::TAG_PTR;
        asm.mov_reg_reg(Reg::R10, Reg::Rax);
        asm.mov_reg_imm64(Reg::R11, heap_mask);
        asm.and_reg_reg(Reg::R10, Reg::R11);
        asm.mov_reg_imm64(Reg::R11, heap_expect);
        asm.cmp_reg_reg(Reg::R10, Reg::R11);
        let p_not_heap = asm.jmp_cond(Cond::NotEqual);

        // 2. Get heap index
        asm.mov_reg_imm64(Reg::R10, 0xFFFF_FFFFu64);
        asm.and_reg_reg(Reg::Rax, Reg::R10);
        asm.mov_reg_mem(Reg::R10, ARG_EXEC_CTX, heap_off as i32);

        // 3. Generation check
        asm.mov_reg_imm64(Reg::R11, 0x8000_0000u64);
        asm.test_reg_reg(Reg::Rax, Reg::R11);
        let p_old = asm.jmp_cond(Cond::NotEqual);

        asm.mov_reg_mem(
            Reg::R10,
            Reg::R10,
            (lay.nursery_slots_vec_off + lay.slots_ptr_off) as i32,
        );
        let p_base_done = asm.jmp_near();

        let old_pos = asm.current_offset();
        asm.patch_u32(p_old, (old_pos as i32 - (p_old as i32 + 4)) as u32);
        asm.mov_reg_imm64(Reg::R11, 0x7FFF_FFFFu64);
        asm.and_reg_reg(Reg::Rax, Reg::R11);
        asm.mov_reg_mem(
            Reg::R10,
            Reg::R10,
            (lay.slots_vec_off + lay.slots_ptr_off) as i32,
        );

        let base_pos = asm.current_offset();
        asm.patch_u32(p_base_done, (base_pos as i32 - (p_base_done as i32 + 4)) as u32);

        asm.mov_reg_imm64(Reg::R11, lay.slot_size as u64);
        asm.imul_reg_reg(Reg::Rax, Reg::R11);
        asm.add_reg_reg(Reg::Rax, Reg::R10);

        // 4. Check tag == HeapObj::Object
        asm.mov_reg_mem(Reg::R10, Reg::Rax, 0);
        asm.mov_reg_imm64(Reg::R11, 0xFF);
        asm.and_reg_reg(Reg::R10, Reg::R11);
        asm.cmp_reg_imm32(Reg::R10, obj_lay.object_tag as i32);
        let p_not_obj = asm.jmp_cond(Cond::NotEqual);

        // 5. Load the object's Rc pointer out of the slot.
        asm.mov_reg_mem(Reg::Rax, Reg::Rax, obj_lay.payload_off as i32);

        // 6. The fields are a tail inside this same allocation, so their base is
        //    a constant displacement — there is no separate buffer to load.
        asm.mov_reg_reg(Reg::R10, Reg::Rax);
        asm.add_reg_imm32(Reg::R10, obj_lay.values_off as i32);
        asm.push(Reg::R10);

        // 7. Load inline_len (u32) into R11. This is the bound that keeps the
        //    fast path off overflowed slots, which live outside the tail and can
        //    only be reached by the interpreter helper.
        asm.mov_reg_mem(Reg::R11, Reg::Rax, obj_lay.len_off as i32);
        asm.mov_reg_imm64(Reg::R10, 0xFFFFFFFFu64);
        asm.and_reg_reg(Reg::R11, Reg::R10);

        // 8. Load shape_id (u32) into Rax
        asm.mov_reg_mem(Reg::Rax, Reg::Rax, obj_lay.shape_off as i32); // shape Rc ptr
        asm.mov_reg_mem(Reg::Rax, Reg::Rax, obj_lay.shape_id_off as i32);
        asm.mov_reg_imm64(Reg::R10, 0xFFFFFFFFu64);
        asm.and_reg_reg(Reg::Rax, Reg::R10); // Rax has shape_id

        // 9. Combine shape_id (Rax) and values_len (R11) into R11 and push it
        asm.shl_reg_imm8(Reg::R11, 32);
        asm.or_reg_reg(Reg::R11, Reg::Rax);
        asm.push(Reg::R11);

        // 10. Load entries array of poly_slot into R10
        asm.mov_reg_mem(Reg::R10, Reg::Rsp, 24);    // Load closure from stack
        asm.mov_reg_mem(Reg::R10, Reg::R10, 40);    // RcBox of ic_cache
        asm.mov_reg_mem(Reg::R10, Reg::R10, 32);    // vec_ptr of ic_cache
        asm.mov_reg_imm64(Reg::R11, cs_idx as u64 * 68); // cs_idx * 68
        asm.add_reg_reg(Reg::R10, Reg::R11);

        // 11. Loop over the 8 entries
        for i in 0..8 {
            // Load shape_id into Rax from [Rsp] (lower 32 bits)
            asm.mov_reg_mem(Reg::Rax, Reg::Rsp, 0);
            asm.shl_reg_imm8(Reg::Rax, 32);
            asm.shr_reg_imm8(Reg::Rax, 32); // Rax has shape_id

            // Load entry u64 into R11
            asm.mov_reg_mem(Reg::R11, Reg::R10, (i * 8) as i32);

            // Extract entry.id (lower 32 bits) into R11
            asm.shl_reg_imm8(Reg::R11, 32);
            asm.shr_reg_imm8(Reg::R11, 32);

            // Check if id != 0
            asm.test_reg_reg(Reg::R11, Reg::R11);
            let p_next = asm.jmp_cond(Cond::Equal);

            // Compare id (R11) with shape_id (Rax)
            asm.cmp_reg_reg(Reg::R11, Reg::Rax);
            let p_next2 = asm.jmp_cond(Cond::NotEqual);

            // Reload entry into R11 to check is_class
            asm.mov_reg_mem(Reg::R11, Reg::R10, (i * 8) as i32);
            asm.shr_reg_imm8(Reg::R11, 48);
            asm.mov_reg_imm64(Reg::Rax, 0xFF);
            asm.and_reg_reg(Reg::R11, Reg::Rax); // R11 has is_class

            // Check if is_class == 1
            asm.cmp_reg_imm32(Reg::R11, 1);
            let p_next3 = asm.jmp_cond(Cond::NotEqual);

            // Match! Reload entry to get slot
            asm.mov_reg_mem(Reg::R11, Reg::R10, (i * 8) as i32);
            asm.shr_reg_imm8(Reg::R11, 32);
            asm.mov_reg_imm64(Reg::Rax, 0xFFFF);
            asm.and_reg_reg(Reg::R11, Reg::Rax); // R11 has slot

            // Verify slot < values_len (upper 32 bits of [Rsp])
            asm.mov_reg_mem(Reg::Rax, Reg::Rsp, 0);
            asm.shr_reg_imm8(Reg::Rax, 32);
            asm.cmp_reg_reg(Reg::R11, Reg::Rax);
            let p_next4 = asm.jmp_cond(Cond::AboveEqual);

            // Match succeeded! Clean stack: pop shape_id/values_len and values_buffer_ptr
            asm.pop(Reg::Rax); // Rax has shape_id/values_len
            asm.pop(Reg::Rax); // Rax has values_buffer_ptr

            // Load values[slot]
            asm.shl_reg_imm8(Reg::R11, 3);
            asm.add_reg_reg(Reg::Rax, Reg::R11);
            asm.mov_reg_mem(Reg::Rax, Reg::Rax, 0);

            emit_store(asm, Reg::Rax, first_reg, regmap);
            done_ic.push(asm.jmp_near());

            let next_pos = asm.current_offset();
            asm.patch_u32(p_next, (next_pos as i32 - (p_next as i32 + 4)) as u32);
            asm.patch_u32(p_next2, (next_pos as i32 - (p_next2 as i32 + 4)) as u32);
            asm.patch_u32(p_next3, (next_pos as i32 - (p_next3 as i32 + 4)) as u32);
            asm.patch_u32(p_next4, (next_pos as i32 - (p_next4 as i32 + 4)) as u32);
        }

        // If loop finished with no match, we pop the pushed values and fall through to slow path
        asm.pop(Reg::Rax);
        asm.pop(Reg::Rax);

        let fallback_pos = asm.current_offset();
        asm.patch_u32(p_oob, (fallback_pos as i32 - (p_oob as i32 + 4)) as u32);
        asm.patch_u32(p_not_heap, (fallback_pos as i32 - (p_not_heap as i32 + 4)) as u32);
        asm.patch_u32(p_not_obj, (fallback_pos as i32 - (p_not_obj as i32 + 4)) as u32);
    }

    emit_load(asm, Reg::Rax, obj_reg, regmap);

    asm.mov_reg_mem(Reg::R11, Reg::Rsp, 8);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = regmap.used_phys.len() % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::R11);
    asm.mov_reg_reg(ARG_BASE, Reg::Rax);
    asm.mov_reg_imm64(ARG_EXEC_CTX, cs_idx as u64);

    asm.mov_reg_imm64(Reg::R10, helpers.get_property_ic_fast as u64);
    asm.call_reg(Reg::R10);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    if need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    asm.mov_reg_reg(Reg::Rax, Reg::R11);

    asm.mov_reg_imm64(Reg::R11, 0x7FF8_0000_0000_0000);
    asm.cmp_reg_reg(Reg::Rax, Reg::R11);

    let fallback_patch = asm.jmp_cond(Cond::Equal);

    emit_store(asm, Reg::Rax, first_reg, regmap);
    let end_patch = asm.jmp_near();

    let fallback_pos = asm.current_offset();
    let relative_fallback = (fallback_pos as i64 - (fallback_patch as i64 + 4)) as i32;
    asm.patch_u32(fallback_patch, relative_fallback as u32);

    emit_flush_all(asm, regmap);

    emit_load(asm, Reg::Rax, obj_reg, regmap);

    asm.mov_reg_mem(ARG_CLOSURE, Reg::Rsp, 8);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let slow_need_dummy = (regmap.used_phys.len() + 5) % 2 == 0;
    if slow_need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_imm64(Reg::R11, *ip as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, first_reg as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, cs_idx as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, name_idx as u64);
    asm.push(Reg::R11);

    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_BASE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.get_property as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    asm.add_reg_imm8(Reg::Rsp, 40);

    if slow_need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::R11, first_reg, regmap);
    emit_reload_all_except(asm, regmap, Some(first_reg));

    let end_pos = asm.current_offset();
    let relative_end = (end_pos as i64 - (end_patch as i64 + 4)) as i32;
    asm.patch_u32(end_patch, relative_end as u32);

    for p in done_ic {
        let disp = (end_pos as i64 - (p as i64 + 4)) as u32;
        asm.patch_u32(p, disp);
    }
}

fn emit_set_property(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let val_reg = (w1 >> 8) as usize;
    let cs_idx = (w1 & 0xFF) as usize;
    let name_idx = code[*ip] as usize;
    *ip += 1;
    let obj_reg = first_reg;

    let lay = helpers.array_layout;
    let obj_lay = helpers.object_layout;
    let heap_off = helpers.heap_field_offset;

    // Fast path: monomorphic-shape slot store into a NURSERY instance.
    // Mirrors emit_get_property's inline IC scan, restricted to
    // is_class == 1 entries (plain preallocated field slots — setters and
    // shape transitions go to the helper). Nursery parents never need the
    // generational write barrier, so old-gen receivers divert to the slow
    // path, same policy as emit_set_index.
    let mut done_ic: Vec<usize> = Vec::new();
    {
        // 0. Check cs_idx < closure.ic_cache_len()
        asm.mov_reg_mem(Reg::R10, Reg::Rsp, 8); // Load closure from stack
        asm.mov_reg_mem(Reg::R10, Reg::R10, 40); // RcBox of ic_cache
        asm.mov_reg_mem(Reg::R11, Reg::R10, 40); // ic_cache_len
        asm.cmp_reg_imm32(Reg::R11, cs_idx as i32);
        let p_oob = asm.jmp_cond(Cond::BelowEqual);

        emit_load(asm, Reg::Rax, obj_reg, regmap);

        // 1. Check if obj is a heap pointer
        let heap_mask = varn_types::vm_value::SIGN
            | varn_types::vm_value::QNAN
            | varn_types::vm_value::MASK_TAG;
        let heap_expect = varn_types::vm_value::SIGN
            | varn_types::vm_value::QNAN
            | varn_types::vm_value::TAG_PTR;
        asm.mov_reg_reg(Reg::R10, Reg::Rax);
        asm.mov_reg_imm64(Reg::R11, heap_mask);
        asm.and_reg_reg(Reg::R10, Reg::R11);
        asm.mov_reg_imm64(Reg::R11, heap_expect);
        asm.cmp_reg_reg(Reg::R10, Reg::R11);
        let p_not_heap = asm.jmp_cond(Cond::NotEqual);

        // 2. Get heap index
        asm.mov_reg_imm64(Reg::R10, 0xFFFF_FFFFu64);
        asm.and_reg_reg(Reg::Rax, Reg::R10);
        asm.mov_reg_mem(Reg::R10, ARG_EXEC_CTX, heap_off as i32);

        // 3. Nursery-only: old-gen parents need the write barrier → slow.
        asm.mov_reg_imm64(Reg::R11, 0x8000_0000u64);
        asm.test_reg_reg(Reg::Rax, Reg::R11);
        let p_old_gen = asm.jmp_cond(Cond::NotEqual);

        asm.mov_reg_mem(
            Reg::R10,
            Reg::R10,
            (lay.nursery_slots_vec_off + lay.slots_ptr_off) as i32,
        );

        asm.mov_reg_imm64(Reg::R11, lay.slot_size as u64);
        asm.imul_reg_reg(Reg::Rax, Reg::R11);
        asm.add_reg_reg(Reg::Rax, Reg::R10);

        // 4. Check tag == HeapObj::Object
        asm.mov_reg_mem(Reg::R10, Reg::Rax, 0);
        asm.mov_reg_imm64(Reg::R11, 0xFF);
        asm.and_reg_reg(Reg::R10, Reg::R11);
        asm.cmp_reg_imm32(Reg::R10, obj_lay.object_tag as i32);
        let p_not_obj = asm.jmp_cond(Cond::NotEqual);

        // 5. Load the object's Rc pointer out of the slot.
        asm.mov_reg_mem(Reg::Rax, Reg::Rax, obj_lay.payload_off as i32);

        // 6. The fields are a tail inside this same allocation, so their base is
        //    a constant displacement — there is no separate buffer to load.
        asm.mov_reg_reg(Reg::R10, Reg::Rax);
        asm.add_reg_imm32(Reg::R10, obj_lay.values_off as i32);
        asm.push(Reg::R10);

        // 7. Load inline_len (u32) into R11. This is the bound that keeps the
        //    fast path off overflowed slots, which live outside the tail and can
        //    only be reached by the interpreter helper.
        asm.mov_reg_mem(Reg::R11, Reg::Rax, obj_lay.len_off as i32);
        asm.mov_reg_imm64(Reg::R10, 0xFFFFFFFFu64);
        asm.and_reg_reg(Reg::R11, Reg::R10);

        // 8. Load shape_id (u32) into Rax
        asm.mov_reg_mem(Reg::Rax, Reg::Rax, obj_lay.shape_off as i32); // shape Rc ptr
        asm.mov_reg_mem(Reg::Rax, Reg::Rax, obj_lay.shape_id_off as i32);
        asm.mov_reg_imm64(Reg::R10, 0xFFFFFFFFu64);
        asm.and_reg_reg(Reg::Rax, Reg::R10); // Rax has shape_id

        // 9. Combine shape_id (Rax) and values_len (R11) into R11 and push it
        asm.shl_reg_imm8(Reg::R11, 32);
        asm.or_reg_reg(Reg::R11, Reg::Rax);
        asm.push(Reg::R11);

        // 10. Load entries array of poly_slot into R10
        asm.mov_reg_mem(Reg::R10, Reg::Rsp, 24); // Load closure from stack
        asm.mov_reg_mem(Reg::R10, Reg::R10, 40); // RcBox of ic_cache
        asm.mov_reg_mem(Reg::R10, Reg::R10, 32); // vec_ptr of ic_cache
        asm.mov_reg_imm64(Reg::R11, cs_idx as u64 * 68); // cs_idx * 68
        asm.add_reg_reg(Reg::R10, Reg::R11);

        // 11. Loop over the 8 entries
        for i in 0..8 {
            // Load shape_id into Rax from [Rsp] (lower 32 bits)
            asm.mov_reg_mem(Reg::Rax, Reg::Rsp, 0);
            asm.shl_reg_imm8(Reg::Rax, 32);
            asm.shr_reg_imm8(Reg::Rax, 32); // Rax has shape_id

            // Load entry u64 into R11
            asm.mov_reg_mem(Reg::R11, Reg::R10, (i * 8) as i32);

            // Extract entry.id (lower 32 bits) into R11
            asm.shl_reg_imm8(Reg::R11, 32);
            asm.shr_reg_imm8(Reg::R11, 32);

            // Check if id != 0
            asm.test_reg_reg(Reg::R11, Reg::R11);
            let p_next = asm.jmp_cond(Cond::Equal);

            // Compare id (R11) with shape_id (Rax)
            asm.cmp_reg_reg(Reg::R11, Reg::Rax);
            let p_next2 = asm.jmp_cond(Cond::NotEqual);

            // Reload entry into R11 to check is_class
            asm.mov_reg_mem(Reg::R11, Reg::R10, (i * 8) as i32);
            asm.shr_reg_imm8(Reg::R11, 48);
            asm.mov_reg_imm64(Reg::Rax, 0xFF);
            asm.and_reg_reg(Reg::R11, Reg::Rax); // R11 has is_class

            // Check if is_class == 1 (plain slot store, no setter)
            asm.cmp_reg_imm32(Reg::R11, 1);
            let p_next3 = asm.jmp_cond(Cond::NotEqual);

            // Match! Reload entry to get slot
            asm.mov_reg_mem(Reg::R11, Reg::R10, (i * 8) as i32);
            asm.shr_reg_imm8(Reg::R11, 32);
            asm.mov_reg_imm64(Reg::Rax, 0xFFFF);
            asm.and_reg_reg(Reg::R11, Reg::Rax); // R11 has slot

            // Verify slot < values_len (upper 32 bits of [Rsp])
            asm.mov_reg_mem(Reg::Rax, Reg::Rsp, 0);
            asm.shr_reg_imm8(Reg::Rax, 32);
            asm.cmp_reg_reg(Reg::R11, Reg::Rax);
            let p_next4 = asm.jmp_cond(Cond::AboveEqual);

            // Match succeeded! Clean stack: pop shape_id/values_len and
            // values_buffer_ptr, then store val into values[slot].
            asm.pop(Reg::Rax); // Rax has shape_id/values_len (discard)
            asm.pop(Reg::Rax); // Rax has values_buffer_ptr

            asm.shl_reg_imm8(Reg::R11, 3);
            asm.add_reg_reg(Reg::Rax, Reg::R11);
            emit_load(asm, Reg::R10, val_reg, regmap);
            asm.mov_mem_reg(Reg::Rax, 0, Reg::R10);
            done_ic.push(asm.jmp_near());

            let next_pos = asm.current_offset();
            asm.patch_u32(p_next, (next_pos as i32 - (p_next as i32 + 4)) as u32);
            asm.patch_u32(p_next2, (next_pos as i32 - (p_next2 as i32 + 4)) as u32);
            asm.patch_u32(p_next3, (next_pos as i32 - (p_next3 as i32 + 4)) as u32);
            asm.patch_u32(p_next4, (next_pos as i32 - (p_next4 as i32 + 4)) as u32);
        }

        // No entry matched: pop the pushed values, fall through to slow path.
        asm.pop(Reg::Rax);
        asm.pop(Reg::Rax);

        let fallback_pos = asm.current_offset();
        asm.patch_u32(p_oob, (fallback_pos as i32 - (p_oob as i32 + 4)) as u32);
        asm.patch_u32(p_not_heap, (fallback_pos as i32 - (p_not_heap as i32 + 4)) as u32);
        asm.patch_u32(p_old_gen, (fallback_pos as i32 - (p_old_gen as i32 + 4)) as u32);
        asm.patch_u32(p_not_obj, (fallback_pos as i32 - (p_not_obj as i32 + 4)) as u32);
    }

    emit_flush_all(asm, regmap);

    emit_load(asm, Reg::Rax, obj_reg, regmap);
    emit_load(asm, Reg::R11, val_reg, regmap);

    asm.mov_reg_mem(ARG_CLOSURE, Reg::Rsp, 8);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 5) % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_imm64(Reg::R10, *ip as u64);
    asm.push(Reg::R10);

    asm.mov_reg_imm64(Reg::R10, cs_idx as u64);
    asm.push(Reg::R10);

    asm.mov_reg_imm64(Reg::R10, name_idx as u64);
    asm.push(Reg::R10);

    asm.push(Reg::R11);

    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_BASE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.set_property as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.add_reg_imm8(Reg::Rsp, 40);

    if need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_reload_all(asm, regmap);

    let end_pos = asm.current_offset();
    for p in done_ic {
        asm.patch_u32(p, (end_pos as i32 - (p as i32 + 4)) as u32);
    }
}
