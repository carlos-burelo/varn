use crate::codegen::ffi::{emit_ffi_call, FfiArg, FfiCallSpec};
use crate::codegen::CodegenCtx;
use crate::assembler::{Cond, Reg};
use crate::regalloc::{emit_load, emit_store};
use crate::registers::ARG_EXEC_CTX;

pub(crate) fn emit_build_object(ctx: &mut CodegenCtx, _first_reg: usize) {
    // The helper re-decodes the pair list from the bytecode ip.
    let ip_before = ctx.ip;
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let dest = (w1 >> 8) as usize;
    let count = (w1 & 0xFF) as usize;
    ctx.ip += count * 2;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.build_object,
            args: &[FfiArg::Imm(ip_before as u64)],
            flush: true,
            dest: Some(dest),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_object_rest(ctx: &mut CodegenCtx, _first_reg: usize) {
    let ip_before = ctx.ip;
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let dest = (w1 >> 8) as usize;
    let w2 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let skip_count = (w2 >> 8) as usize;
    ctx.ip += skip_count;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.object_rest,
            args: &[FfiArg::Imm(ip_before as u64)],
            flush: true,
            dest: Some(dest),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_get_fixed_field(ctx: &mut CodegenCtx, first_reg: usize) {
    let obj_reg = (ctx.code[ctx.ip] >> 8) as usize;
    ctx.ip += 1;
    let slot = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    let lay = ctx.helpers.array_layout;
    let obj_lay = ctx.helpers.object_layout;
    let heap_off = ctx.helpers.heap_field_offset;

    let mut slow: Vec<usize> = Vec::new();
    {
        let asm = &mut ctx.asm;
        let regmap = &ctx.regmap;

        emit_load(asm, Reg::Rax, obj_reg, regmap);

        // heap check
        let heap_mask =
            varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::MASK_TAG;
        let heap_expect =
            varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::TAG_PTR;
        asm.mov_reg_reg(Reg::R10, Reg::Rax);
        asm.mov_reg_imm64(Reg::Rcx, heap_mask);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.mov_reg_imm64(Reg::Rcx, heap_expect);
        asm.cmp_reg_reg(Reg::R10, Reg::Rcx);
        slow.push(asm.jmp_cond(Cond::NotEqual));

        // Get heap index
        asm.mov_reg_imm64(Reg::Rcx, 0xFFFF_FFFFu64);
        asm.and_reg_reg(Reg::Rax, Reg::Rcx);
        asm.mov_reg_mem(Reg::Rcx, ARG_EXEC_CTX, heap_off as i32);

        asm.mov_reg_imm64(Reg::R10, 0x8000_0000u64);
        asm.test_reg_reg(Reg::Rax, Reg::R10);
        let p_old = asm.jmp_cond(Cond::NotEqual);

        asm.mov_reg_mem(
            Reg::Rcx,
            Reg::Rcx,
            (lay.nursery_slots_vec_off + lay.slots_ptr_off) as i32,
        );
        let p_base_done = asm.jmp_near();

        let old_pos = asm.current_offset();
        asm.patch_u32(p_old, (old_pos as i32 - (p_old as i32 + 4)) as u32);
        asm.mov_reg_imm64(Reg::R10, 0x7FFF_FFFFu64);
        asm.and_reg_reg(Reg::Rax, Reg::R10);
        asm.mov_reg_mem(
            Reg::Rcx,
            Reg::Rcx,
            (lay.slots_vec_off + lay.slots_ptr_off) as i32,
        );

        let base_pos = asm.current_offset();
        asm.patch_u32(p_base_done, (base_pos as i32 - (p_base_done as i32 + 4)) as u32);

        asm.mov_reg_imm64(Reg::R10, lay.slot_size as u64);
        asm.imul_reg_reg(Reg::Rax, Reg::R10);
        asm.add_reg_reg(Reg::Rax, Reg::Rcx);

        // Check if it is an Object
        asm.mov_reg_mem(Reg::R10, Reg::Rax, 0);
        asm.mov_reg_imm64(Reg::Rcx, 0xFF);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.cmp_reg_imm32(Reg::R10, obj_lay.object_tag as i32);
        slow.push(asm.jmp_cond(Cond::NotEqual));

        // Load the object's Rc pointer out of the slot.
        asm.mov_reg_mem(Reg::Rax, Reg::Rax, obj_lay.payload_off as i32);

        // Verify slot < inline_len. Fields past the tail spilled to the overflow
        // store, which only the interpreter helper knows how to read.
        asm.mov_reg_mem(Reg::R10, Reg::Rax, obj_lay.len_off as i32);
        asm.mov_reg_imm64(Reg::Rcx, 0xFFFFFFFFu64);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.cmp_reg_imm32(Reg::R10, slot as i32);
        slow.push(asm.jmp_cond(Cond::BelowEqual));

        // The fields are inline in this same allocation: one constant-offset load.
        asm.mov_reg_mem(Reg::Rax, Reg::Rax, (obj_lay.values_off + slot * 8) as i32);

        emit_store(asm, Reg::Rax, first_reg, regmap);
    }
    let done = ctx.asm.jmp_near();

    let slow_pos = ctx.asm.current_offset();
    for p in slow {
        ctx.asm.patch_u32(p, (slow_pos as i32 - (p as i32 + 4)) as u32);
    }

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.get_fixed_field,
            args: &[FfiArg::Vreg(obj_reg), FfiArg::Imm(slot as u64)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: true,
        },
    );

    let end = ctx.asm.current_offset();
    ctx.asm
        .patch_u32(done, (end as i32 - (done as i32 + 4)) as u32);
}

pub(crate) fn emit_set_fixed_field(ctx: &mut CodegenCtx, first_reg: usize) {
    let val_reg = (ctx.code[ctx.ip] >> 8) as usize;
    ctx.ip += 1;
    let slot = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;
    let obj_reg = first_reg;

    let lay = ctx.helpers.array_layout;
    let obj_lay = ctx.helpers.object_layout;
    let heap_off = ctx.helpers.heap_field_offset;

    let mut slow: Vec<usize> = Vec::new();
    {
        let asm = &mut ctx.asm;
        let regmap = &ctx.regmap;

        emit_load(asm, Reg::Rax, obj_reg, regmap);

        // heap check
        let heap_mask =
            varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::MASK_TAG;
        let heap_expect =
            varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::TAG_PTR;
        asm.mov_reg_reg(Reg::R10, Reg::Rax);
        asm.mov_reg_imm64(Reg::Rcx, heap_mask);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.mov_reg_imm64(Reg::Rcx, heap_expect);
        asm.cmp_reg_reg(Reg::R10, Reg::Rcx);
        slow.push(asm.jmp_cond(Cond::NotEqual));

        // Get heap index
        asm.mov_reg_imm64(Reg::Rcx, 0xFFFF_FFFFu64);
        asm.and_reg_reg(Reg::Rax, Reg::Rcx);

        // Check if nursery: if bit 31 is set, it's old gen. Fall back so write barrier runs!
        asm.mov_reg_imm64(Reg::R10, 0x8000_0000u64);
        asm.test_reg_reg(Reg::Rax, Reg::R10);
        slow.push(asm.jmp_cond(Cond::NotEqual));

        // Nursery gen:
        asm.mov_reg_mem(Reg::Rcx, ARG_EXEC_CTX, heap_off as i32);
        asm.mov_reg_mem(
            Reg::Rcx,
            Reg::Rcx,
            (lay.nursery_slots_vec_off + lay.slots_ptr_off) as i32,
        );

        asm.mov_reg_imm64(Reg::R10, lay.slot_size as u64);
        asm.imul_reg_reg(Reg::Rax, Reg::R10);
        asm.add_reg_reg(Reg::Rax, Reg::Rcx);

        // Check if it is an Object
        asm.mov_reg_mem(Reg::R10, Reg::Rax, 0);
        asm.mov_reg_imm64(Reg::Rcx, 0xFF);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.cmp_reg_imm32(Reg::R10, obj_lay.object_tag as i32);
        slow.push(asm.jmp_cond(Cond::NotEqual));

        // Load the object's Rc pointer out of the slot.
        asm.mov_reg_mem(Reg::Rax, Reg::Rax, obj_lay.payload_off as i32);

        // Verify slot < inline_len. Fields past the tail spilled to the overflow
        // store, which only the interpreter helper knows how to write.
        asm.mov_reg_mem(Reg::R10, Reg::Rax, obj_lay.len_off as i32);
        asm.mov_reg_imm64(Reg::Rcx, 0xFFFFFFFFu64);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.cmp_reg_imm32(Reg::R10, slot as i32);
        slow.push(asm.jmp_cond(Cond::BelowEqual));

        // The fields are inline in this same allocation: one constant-offset store.
        emit_load(asm, Reg::R10, val_reg, regmap);
        asm.mov_mem_reg(Reg::Rax, (obj_lay.values_off + slot * 8) as i32, Reg::R10);
    }
    let done = ctx.asm.jmp_near();

    let slow_pos = ctx.asm.current_offset();
    for p in slow {
        ctx.asm.patch_u32(p, (slow_pos as i32 - (p as i32 + 4)) as u32);
    }

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.set_fixed_field,
            args: &[
                FfiArg::Vreg(obj_reg),
                FfiArg::Imm(slot as u64),
                FfiArg::Vreg(val_reg),
            ],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: true,
        },
    );

    let end = ctx.asm.current_offset();
    ctx.asm
        .patch_u32(done, (end as i32 - (done as i32 + 4)) as u32);
}

pub(crate) fn emit_get_property_maybe(ctx: &mut CodegenCtx, first_reg: usize) {
    let obj_reg = (ctx.code[ctx.ip] >> 8) as usize;
    ctx.ip += 1;
    let name_idx = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.get_property_maybe,
            args: &[FfiArg::Vreg(obj_reg), FfiArg::Imm(name_idx as u64)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_get_super(ctx: &mut CodegenCtx, first_reg: usize) {
    let name_idx = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.get_super,
            args: &[FfiArg::Imm(name_idx as u64)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_get_symbol(ctx: &mut CodegenCtx, first_reg: usize) {
    let obj_reg = (ctx.code[ctx.ip] >> 8) as usize;
    ctx.ip += 1;
    let sym_idx = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.get_symbol,
            args: &[FfiArg::Vreg(obj_reg), FfiArg::Imm(sym_idx as u64)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}
