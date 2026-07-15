use varn_core::OpCode;

use crate::assembler::{Cond, Reg};
use crate::regalloc::{
    emit_flush_all, emit_flush_for_call, emit_load, emit_reload_after_call, emit_reload_all_except,
    emit_store,
};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

/// Maximum self-call depth before the JIT diverts to the helper that raises a
/// catchable runtime error. Kept in sync with the interpreter / `ctx_jit_values`
/// guard (`MAX_CALL_DEPTH = 10000`).
const MAX_JIT_CALL_DEPTH: usize = 10000;

pub(crate) fn emit_calls(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) -> Result<(), String> {
    match op {
        OpCode::Call => emit_call(ctx, first_reg),
        OpCode::CallSelf => emit_call_self(ctx, first_reg),
        OpCode::CallMethod => emit_call_method(ctx, first_reg),
        OpCode::InvokeVirtual => emit_invoke_virtual(ctx),
        OpCode::Intrinsic => emit_intrinsic(ctx, first_reg),
        OpCode::CallNativeOp => emit_call_native_op(ctx, first_reg),
        _ => unreachable!("emit_calls called with {:?}", op),
    }
    Ok(())
}

fn emit_call(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let w2 = code[*ip];
    *ip += 1;
    let dest = (w1 >> 8) as usize;
    let callee_reg = (w1 & 0xFF) as usize;
    let arg_count = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    let _ = first_reg;

    use crate::assembler::Cond;

    emit_flush_all(asm, regmap);

    emit_load(asm, Reg::Rax, callee_reg, regmap);

    let need_prepare_dummy = regmap.used_phys.len() % 2 != 0;
    if need_prepare_dummy {
        asm.push(Reg::Rax);
    }
    asm.push(ARG_CTX);
    asm.push(ARG_EXEC_CTX);
    asm.push(ARG_BASE);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.add_reg_imm32(ARG_BASE, arg_start as i32);
    asm.mov_reg_imm64(ARG_EXEC_CTX, arg_count as u64);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.jit_prepare_call as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.cmp_reg_imm32(Reg::Rax, 1);
    let native_patch = asm.jmp_cond(Cond::Equal);

    asm.cmp_reg_imm32(Reg::Rax, 0);
    let fallback_patch = asm.jmp_cond(Cond::Equal);

    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);
    if need_prepare_dummy {
        asm.pop(Reg::R11);
    }

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    asm.push(ARG_CTX);
    asm.push(ARG_EXEC_CTX);
    asm.push(ARG_BASE);

    let need_align = regmap.used_phys.len() % 2 != 0;
    if need_align {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_reg(Reg::R11, ARG_BASE);
    asm.add_reg_imm32(Reg::R11, arg_start as i32);

    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);

    asm.mov_reg_reg(ARG_BASE, Reg::R11);

    asm.mov_reg_mem(Reg::R10, ARG_CLOSURE, 56);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    if need_align {
        asm.pop(Reg::R11);
    }

    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);

    let need_dummy = regmap.used_phys.len() % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_reg(ARG_CLOSURE, ARG_BASE);
    asm.add_reg_imm32(ARG_CLOSURE, arg_start as i32);

    asm.mov_reg_reg(ARG_BASE, Reg::Rax);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.jit_post_call as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    if need_dummy {
        asm.pop(Reg::R11);
    }

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::Rax, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
    let end_patch = asm.jmp_near();

    // Native JIT sentinel path (Rax was 1)
    let native_pos = asm.current_offset();
    let relative_native = (native_pos as i64 - (native_patch as i64 + 4)) as i32;
    asm.patch_u32(native_patch, relative_native as u32);

    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);
    if need_prepare_dummy {
        asm.pop(Reg::R11);
    }

    // Load result from ExecCtx.jit_native_result
    asm.mov_reg_mem(Reg::Rax, ARG_EXEC_CTX, helpers.jit_native_result_offset as i32);

    emit_store(asm, Reg::Rax, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
    let end_native_patch_sentinel = asm.jmp_near();

    emit_load(asm, Reg::Rax, callee_reg, regmap);
    let run_native_patch = asm.jmp_near();

    // Fallback path (Rax was 0)
    let fallback_pos = asm.current_offset();
    let relative_fallback = (fallback_pos as i64 - (fallback_patch as i64 + 4)) as i32;
    asm.patch_u32(fallback_patch, relative_fallback as u32);

    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);
    if need_prepare_dummy {
        asm.pop(Reg::R11);
    }

    // Since Rax was overwritten by the 0 return of jit_prepare_call, reload callee:
    emit_load(asm, Reg::Rax, callee_reg, regmap);

    let need_is_native_dummy = regmap.used_phys.len() % 2 == 0;
    if need_is_native_dummy {
        asm.push(Reg::Rax);
    }
    asm.push(ARG_CTX);
    asm.push(ARG_EXEC_CTX);
    asm.push(ARG_BASE);
    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.jit_is_native_fn as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.cmp_reg_imm32(Reg::Rax, 0);
    let not_native_patch = asm.jmp_cond(Cond::Equal);

    asm.pop(Reg::Rax);
    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);
    if need_is_native_dummy {
        asm.pop(Reg::R11);
    }

    // Run native path (both direct native sentinel jump and resolved native fallthrough land here)
    let run_native_pos = asm.current_offset();
    let relative_run_native = (run_native_pos as i64 - (run_native_patch as i64 + 4)) as i32;
    asm.patch_u32(run_native_patch, relative_run_native as u32);

    let need_native_call_dummy = regmap.used_phys.len() % 2 != 0;
    if need_native_call_dummy {
        asm.push(Reg::Rax);
    }
    asm.push(ARG_CTX);
    asm.push(ARG_EXEC_CTX);
    asm.push(ARG_BASE);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_imm64(ARG_BASE, arg_start as u64);
    asm.mov_reg_imm64(ARG_EXEC_CTX, arg_count as u64);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.jit_call_native_fast as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);
    if need_native_call_dummy {
        asm.pop(Reg::R11);
    }

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::Rax, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
    let end_native_patch = asm.jmp_near();

    let not_native_pos = asm.current_offset();
    let disp_not_native = (not_native_pos as i64 - (not_native_patch as i64 + 4)) as i32;
    asm.patch_u32(not_native_patch, disp_not_native as u32);

    asm.pop(Reg::Rax);
    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);
    if need_is_native_dummy {
        asm.pop(Reg::R11);
    }

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_load(asm, Reg::Rax, callee_reg, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 5) % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_imm64(Reg::R11, *ip as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, dest as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, arg_count as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, arg_start as u64);
    asm.push(Reg::R11);

    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.call as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

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

    emit_store(asm, Reg::R11, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));

    let end_pos = asm.current_offset();
    let relative_end = (end_pos as i64 - (end_patch as i64 + 4)) as i32;
    asm.patch_u32(end_patch, relative_end as u32);

    let relative_end_native = (end_pos as i64 - (end_native_patch as i64 + 4)) as i32;
    asm.patch_u32(end_native_patch, relative_end_native as u32);

    let relative_end_sentinel = (end_pos as i64 - (end_native_patch_sentinel as i64 + 4)) as i32;
    asm.patch_u32(end_native_patch_sentinel, relative_end_sentinel as u32);
}

fn emit_call_self(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;
    let meta = &ctx.proto.register_meta;
    let safe_opt = ctx.safe_int_call_opt;

    let w1 = code[*ip];
    *ip += 1;
    let w2 = code[*ip];
    *ip += 1;
    let dest = (w1 >> 8) as usize;
    let callee_dummy = (w1 & 0xFF) as usize;
    let arg_count = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    let _ = first_reg;
    let _ = callee_dummy;

    // Immediate (`int`/`float`/`bool`) locals that aren't arguments stay in their
    // callee-saved registers across the recursive call; only pointer slots and the
    // arguments the callee reads from memory are spilled. See `emit_flush_for_call`.
    emit_flush_for_call(
        asm,
        regmap,
        meta,
        safe_opt,
        arg_start,
        arg_start + arg_count,
    );

    // NOTE: a former "is_pure" fast path emitted a bare native `call` to self with
    // no frame push and no depth check, so deep self-recursion overflowed the host
    // stack (hard abort). All self-calls now go through the frame-tracking path
    // below, which enforces the call-depth guard (see `fallback_depth`).
    let register_count = ctx.proto.register_count as usize;

    asm.mov_reg_mem(Reg::R10, ARG_EXEC_CTX, 0);

    asm.mov_reg_reg(Reg::R11, ARG_BASE);
    asm.add_reg_imm32(Reg::R11, (arg_start + register_count + 32) as i32);

    asm.cmp_reg_reg(Reg::R10, Reg::R11);
    let fallback_grow = asm.jmp_cond(Cond::Less);

    asm.mov_reg_mem(Reg::R10, ARG_EXEC_CTX, 40);
    asm.mov_reg_mem(Reg::R11, ARG_EXEC_CTX, 24);
    asm.cmp_reg_reg(Reg::R10, Reg::R11);
    let fallback_grow_frames = asm.jmp_cond(Cond::GreaterEqual);

    // Call-depth guard: when frames.len() reaches the limit, divert to the helper
    // path (jit_push_self_frame), which raises a catchable runtime error rather
    // than letting native self-recursion overflow the host stack.
    asm.mov_reg_mem(Reg::R10, ARG_EXEC_CTX, 40);
    asm.cmp_reg_imm32(Reg::R10, MAX_JIT_CALL_DEPTH as i32);
    let fallback_depth = asm.jmp_cond(Cond::GreaterEqual);

    if !safe_opt {
        asm.mov_reg_reg(Reg::R10, ARG_BASE);
        asm.add_reg_imm32(Reg::R10, (arg_start + register_count) as i32);
        asm.mov_mem_reg(ARG_EXEC_CTX, 16, Reg::R10);

        asm.mov_reg_mem(Reg::R10, ARG_EXEC_CTX, 40);
        asm.mov_reg_imm64(Reg::R11, 48);
        asm.imul_reg_reg(Reg::R10, Reg::R11);
        asm.mov_reg_mem(Reg::R11, ARG_EXEC_CTX, 32);
        asm.add_reg_reg(Reg::R10, Reg::R11);

        asm.mov_mem_reg(Reg::R10, 0, ARG_CLOSURE);
        asm.xor_reg_reg(Reg::R11, Reg::R11);
        asm.mov_mem_reg(Reg::R10, 8, Reg::R11);
        asm.mov_mem_reg(Reg::R10, 16, Reg::R11);

        asm.mov_reg_reg(Reg::R11, ARG_BASE);
        asm.add_reg_imm32(Reg::R11, arg_start as i32);
        asm.mov_mem_reg(Reg::R10, 24, Reg::R11);

        asm.xor_reg_reg(Reg::R11, Reg::R11);
        asm.mov_mem_reg(Reg::R10, 32, Reg::R11);
        asm.mov_mem_reg(Reg::R10, 40, Reg::R11);
    }

    asm.mov_reg_mem(Reg::R10, ARG_EXEC_CTX, 40);
    asm.add_reg_imm8(Reg::R10, 1);
    asm.mov_mem_reg(ARG_EXEC_CTX, 40, Reg::R10);

    let skip_fallback = asm.jmp_near();

    let fallback_pos = asm.current_offset();
    let disp_fallback_grow = (fallback_pos as i64 - (fallback_grow as i64 + 4)) as i32;
    asm.patch_u32(fallback_grow, disp_fallback_grow as u32);
    let disp_fallback_grow_frames =
        (fallback_pos as i64 - (fallback_grow_frames as i64 + 4)) as i32;
    asm.patch_u32(fallback_grow_frames, disp_fallback_grow_frames as u32);
    let disp_fallback_depth = (fallback_pos as i64 - (fallback_depth as i64 + 4)) as i32;
    asm.patch_u32(fallback_depth, disp_fallback_depth as u32);

    let need_dummy_push = regmap.used_phys.len() % 2 != 0;
    if need_dummy_push {
        asm.push(Reg::Rax);
    }
    asm.push(ARG_CTX);
    asm.push(ARG_EXEC_CTX);
    asm.push(ARG_BASE);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, ARG_BASE);
    asm.add_reg_imm32(ARG_CLOSURE, arg_start as i32);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.jit_push_self_frame as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);
    if need_dummy_push {
        asm.pop(Reg::Rax);
    }

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    let end_fast_path_pos = asm.current_offset();
    let disp_skip_fallback = (end_fast_path_pos as i64 - (skip_fallback as i64 + 4)) as i32;
    asm.patch_u32(skip_fallback, disp_skip_fallback as u32);

    asm.mov_reg_mem(ARG_CLOSURE, Reg::Rsp, 8);

    asm.push(ARG_CTX);
    asm.push(ARG_EXEC_CTX);
    asm.push(ARG_BASE);

    let need_align = regmap.used_phys.len() % 2 != 0;
    if need_align {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_reg(Reg::R11, ARG_BASE);
    asm.add_reg_imm32(Reg::R11, arg_start as i32);

    asm.mov_reg_reg(ARG_BASE, Reg::R11);

    let call_patch = asm.call_near();
    let displacement = (0 as isize - (call_patch + 4) as isize) as i32;
    asm.patch_u32(call_patch, displacement as u32);

    if need_align {
        asm.pop(Reg::R11);
    }

    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);

    if safe_opt {
        asm.mov_reg_mem(Reg::R10, ARG_EXEC_CTX, 40);
        asm.add_reg_imm8(Reg::R10, -1);
        asm.mov_mem_reg(ARG_EXEC_CTX, 40, Reg::R10);
    } else {
        asm.mov_reg_mem(
            Reg::R10,
            ARG_EXEC_CTX,
            (helpers.open_upvalues_offset + 16) as i32,
        );
        asm.cmp_reg_imm32(Reg::R10, 0);
        let fallback_post = asm.jmp_cond(Cond::NotEqual);

        asm.mov_reg_mem(
            Reg::R10,
            ARG_EXEC_CTX,
            (helpers.pending_constructors_offset + 16) as i32,
        );
        asm.cmp_reg_imm32(Reg::R10, 0);
        let fallback_post_2 = asm.jmp_cond(Cond::NotEqual);

        asm.mov_reg_mem(Reg::R10, ARG_EXEC_CTX, 40);
        asm.add_reg_imm8(Reg::R10, -1);
        asm.mov_mem_reg(ARG_EXEC_CTX, 40, Reg::R10);

        let skip_fallback_post = asm.jmp_near();

        let fallback_post_pos = asm.current_offset();
        let disp_fallback_post = (fallback_post_pos as i64 - (fallback_post as i64 + 4)) as i32;
        asm.patch_u32(fallback_post, disp_fallback_post as u32);
        let disp_fallback_post_2 = (fallback_post_pos as i64 - (fallback_post_2 as i64 + 4)) as i32;
        asm.patch_u32(fallback_post_2, disp_fallback_post_2 as u32);

        asm.push(ARG_BASE);
        asm.push(ARG_EXEC_CTX);

        asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
        asm.mov_reg_reg(ARG_CLOSURE, ARG_BASE);
        asm.add_reg_imm32(ARG_CLOSURE, arg_start as i32);
        asm.mov_reg_reg(ARG_BASE, Reg::Rax);

        #[cfg(target_os = "windows")]
        asm.add_reg_imm8(Reg::Rsp, -32);

        asm.mov_reg_imm64(Reg::R10, helpers.jit_post_call as u64);
        asm.call_reg(Reg::R10);

        #[cfg(target_os = "windows")]
        asm.add_reg_imm8(Reg::Rsp, 32);

        asm.pop(ARG_EXEC_CTX);
        asm.pop(ARG_BASE);

        let end_fast_path_post_pos = asm.current_offset();
        let disp_skip_fallback_post =
            (end_fast_path_post_pos as i64 - (skip_fallback_post as i64 + 4)) as i32;
        asm.patch_u32(skip_fallback_post, disp_skip_fallback_post as u32);
    }

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::Rax, dest, regmap);
    // Immediate locals survived in callee-saved registers; only reload pointer
    // slots (a copying GC in the callee may have relocated them).
    emit_reload_after_call(asm, regmap, meta, safe_opt, Some(dest));
}

fn emit_call_method(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let cs = first_reg;
    let w1 = code[*ip];
    *ip += 1;
    let name_idx = code[*ip] as usize;
    *ip += 1;
    let w3 = code[*ip];
    *ip += 1;
    let dest = (w1 >> 8) as usize;
    let obj_reg = (w1 & 0xFF) as usize;
    let arg_count = (w3 >> 8) as usize;
    let arg_start = (w3 & 0xFF) as usize;

    emit_flush_all(asm, regmap);

    emit_load(asm, Reg::Rax, obj_reg, regmap);

    asm.mov_reg_mem(ARG_CLOSURE, Reg::Rsp, 8);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 7) % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_imm64(Reg::R11, *ip as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, dest as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, arg_count as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, arg_start as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, cs as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, name_idx as u64);
    asm.push(Reg::R11);

    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_BASE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.call_method as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    asm.add_reg_imm8(Reg::Rsp, 56);

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

    emit_store(asm, Reg::R11, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
}

fn emit_invoke_virtual(ctx: &mut CodegenCtx) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let name_idx = code[*ip] as usize;
    *ip += 1;
    let w3 = code[*ip];
    *ip += 1;
    let dest = (w1 >> 8) as usize;
    let obj_reg = (w1 & 0xFF) as usize;
    let arg_count = (w3 >> 8) as usize;
    let arg_start = (w3 & 0xFF) as usize;

    emit_flush_all(asm, regmap);

    emit_load(asm, Reg::Rax, obj_reg, regmap);

    asm.mov_reg_mem(ARG_CLOSURE, Reg::Rsp, 8);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 6) % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_imm64(Reg::R11, *ip as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, dest as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, arg_count as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, arg_start as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, name_idx as u64);
    asm.push(Reg::R11);

    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_BASE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.invoke_virtual as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    asm.add_reg_imm8(Reg::Rsp, 48);

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

    emit_store(asm, Reg::R11, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
}

fn emit_intrinsic(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let wire_byte = (w1 >> 8) as usize;
    let arg_count = (w1 & 0xFF) as usize;

    let dest = first_reg;

    let value_reg = dest + 1;
    if arg_count == 2
        && value_reg < ctx.proto.register_meta.len()
        && ctx.proto.register_meta[value_reg].kind == varn_types::register_meta::SlotKind::Float
    {
        match wire_byte as u8 {
            0x00 => {
                emit_load(asm, Reg::Rax, value_reg, regmap);
                asm.emit_debug_assert_not_int(Reg::Rax);
                asm.mov_reg_imm64(Reg::Rcx, 0x7FFF_FFFF_FFFF_FFFF);
                asm.and_reg_reg(Reg::Rax, Reg::Rcx);
                emit_store(asm, Reg::Rax, dest, regmap);
                return;
            }
            0x01 => {
                emit_load(asm, Reg::Rax, value_reg, regmap);
                asm.emit_debug_assert_not_int(Reg::Rax);
                asm.movq_xmm_from_gpr(0, Reg::Rax);
                asm.sqrtsd(0, 0);
                asm.movq_gpr_from_xmm(Reg::Rax, 0);
                asm.emit_nan_to_null(Reg::Rax, 0);
                emit_store(asm, Reg::Rax, dest, regmap);
                return;
            }
            0x02 => {
                emit_load(asm, Reg::Rax, value_reg, regmap);
                asm.emit_debug_assert_not_int(Reg::Rax);
                asm.movq_xmm_from_gpr(0, Reg::Rax);
                asm.roundsd(0, 0, 0x09);
                asm.movq_gpr_from_xmm(Reg::Rax, 0);
                asm.emit_nan_to_null(Reg::Rax, 0);
                emit_store(asm, Reg::Rax, dest, regmap);
                return;
            }
            _ => {}
        }
    }

    emit_flush_all(asm, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_EXEC_CTX);
    asm.push(ARG_BASE);

    let need_align = (regmap.used_phys.len() + 4) % 2 != 0;
    if need_align {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(ARG_CLOSURE, wire_byte as u64);

    asm.add_reg_imm32(ARG_BASE, dest as i32);

    asm.mov_reg_imm64(ARG_EXEC_CTX, arg_count as u64);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.dispatch_intrinsic as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    if need_align {
        asm.pop(Reg::R11);
    }

    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::Rax, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
}

fn load_fast_arg(
    asm: &mut crate::assembler::Assembler,
    regmap: &crate::regalloc::RegMap,
    arg_idx: usize,
    vreg: usize,
    arg_type: varn_types::ArgType,
    _next_int_idx: &mut usize,
    _next_float_idx: &mut usize,
) {
    let is_float = arg_type == varn_types::ArgType::Float;
    
    #[cfg(target_os = "windows")]
    {
        let slot = arg_idx;
        if is_float {
            emit_load(asm, Reg::Rax, vreg, regmap);
            asm.movq_xmm_from_gpr(slot as u8, Reg::Rax);
        } else {
            let target_gpr = match slot {
                0 => Reg::Rcx,
                1 => Reg::Rdx,
                2 => Reg::R8,
                3 => Reg::R9,
                _ => panic!("Varn fast-call supports up to 4 arguments on Windows"),
            };
            emit_load(asm, target_gpr, vreg, regmap);
            if arg_type == varn_types::ArgType::Int {
                asm.shl_reg_imm8(target_gpr, 16);
                asm.sar_reg_imm8(target_gpr, 16);
            } else if arg_type == varn_types::ArgType::Bool {
                asm.mov_reg_imm64(Reg::R11, 0x7FFB_0000_0000_0000u64);
                asm.cmp_reg_reg(target_gpr, Reg::R11);
                let patch_true = asm.jmp_cond(crate::assembler::Cond::Equal);
                asm.mov_reg_imm64(target_gpr, 0);
                let patch_end = asm.jmp_near();
                let true_pos = asm.current_offset();
                asm.mov_reg_imm64(target_gpr, 1);
                let end_pos = asm.current_offset();
                let disp_true = (true_pos as i32 - (patch_true as i32 + 4)) as u32;
                asm.patch_u32(patch_true, disp_true);
                let disp_end = (end_pos as i32 - (patch_end as i32 + 4)) as u32;
                asm.patch_u32(patch_end, disp_end);
            }
        }
    }
    
    #[cfg(not(target_os = "windows"))]
    {
        if is_float {
            let slot = *_next_float_idx;
            *_next_float_idx += 1;
            emit_load(asm, Reg::Rax, vreg, regmap);
            asm.movq_xmm_from_gpr(slot as u8, Reg::Rax);
        } else {
            let slot = *_next_int_idx;
            *_next_int_idx += 1;
            let target_gpr = match slot {
                0 => Reg::Rdi,
                1 => Reg::Rsi,
                2 => Reg::Rdx,
                3 => Reg::Rcx,
                4 => Reg::R8,
                5 => Reg::R9,
                _ => panic!("Varn fast-call supports up to 6 GPR arguments on SysV"),
            };
            emit_load(asm, target_gpr, vreg, regmap);
            if arg_type == varn_types::ArgType::Int {
                asm.shl_reg_imm8(target_gpr, 16);
                asm.sar_reg_imm8(target_gpr, 16);
            } else if arg_type == varn_types::ArgType::Bool {
                asm.mov_reg_imm64(Reg::R11, 0x7FFB_0000_0000_0000u64);
                asm.cmp_reg_reg(target_gpr, Reg::R11);
                let patch_true = asm.jmp_cond(crate::assembler::Cond::Equal);
                asm.mov_reg_imm64(target_gpr, 0);
                let patch_end = asm.jmp_near();
                let true_pos = asm.current_offset();
                asm.mov_reg_imm64(target_gpr, 1);
                let end_pos = asm.current_offset();
                let disp_true = (true_pos as i32 - (patch_true as i32 + 4)) as u32;
                asm.patch_u32(patch_true, disp_true);
                let disp_end = (end_pos as i32 - (patch_end as i32 + 4)) as u32;
                asm.patch_u32(patch_end, disp_end);
            }
        }
    }
}

fn emit_call_native_op(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;
    let proto = ctx.proto;

    let cidx = code[*ip] as usize;
    let total = code[*ip + 1] as usize; // receiver + args
    *ip += 2;

    // Op-id is known at JIT-compile time.
    let op_id = match proto.chunk.constants.get(cidx) {
        Some(varn_types::chunk::PoolEntry::Literal(varn_types::chunk::Literal::Int(i))) => {
            *i as u64
        }
        _ => 0,
    };

    let (slow_addr, fast_addr, signature) = (helpers.resolve_native_op_v2)(op_id);
    let dest = first_reg;

    if fast_addr != 0 {
        // --- FAST PATH (Varn ABI v2) ---
        emit_flush_all(asm, regmap);

        // Save caller-saved registers and align stack to 16 bytes.
        asm.push(Reg::Rax); 
        asm.push(ARG_CTX);
        asm.push(ARG_EXEC_CTX);
        asm.push(ARG_BASE);

        let mut next_int_idx = 0usize;
        let mut next_float_idx = 0usize;

        for i in 0..total {
            let arg_vreg = dest + i;
            let arg_type = signature.param_types.get(i).copied().unwrap_or(varn_types::ArgType::Void);
            if arg_type == varn_types::ArgType::Void {
                break;
            }
            load_fast_arg(asm, regmap, i, arg_vreg, arg_type, &mut next_int_idx, &mut next_float_idx);
        }

        #[cfg(target_os = "windows")]
        asm.add_reg_imm8(Reg::Rsp, -32); // shadow space

        asm.mov_reg_imm64(Reg::R10, fast_addr as u64);
        asm.call_reg(Reg::R10);

        #[cfg(target_os = "windows")]
        asm.add_reg_imm8(Reg::Rsp, 32);

        // Pop back caller-saved registers and alignment
        asm.pop(ARG_BASE);
        asm.pop(ARG_EXEC_CTX);
        asm.pop(ARG_CTX);
        asm.pop(Reg::R11); // pop alignment dummy

        // Box the return value in Reg::Rax
        let ret_type = signature.return_type;
        match ret_type {
            varn_types::ArgType::Void => {
                asm.mov_reg_imm64(Reg::Rax, 0x7FF9_0000_0000_0000u64); // null
            }
            varn_types::ArgType::Int => {
                // Mask to 48-bit and OR with TAG_INT
                asm.push(Reg::R11);
                asm.mov_reg_imm64(Reg::R11, 0x0000_FFFF_FFFF_FFFFu64);
                asm.and_reg_reg(Reg::Rax, Reg::R11);
                asm.pop(Reg::R11);
                asm.or_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);
            }
            varn_types::ArgType::Float => {
                // Move float from Xmm0 back to Rax
                asm.movq_gpr_from_xmm(Reg::Rax, 0);
            }
            varn_types::ArgType::Bool => {
                // Box returned 0/1 back to bool VmValue
                asm.cmp_reg_imm32(Reg::Rax, 1);
                let patch_true = asm.jmp_cond(crate::assembler::Cond::Equal);
                
                // False path
                asm.mov_reg_imm64(Reg::Rax, 0x7FFA_0000_0000_0000u64);
                let patch_end = asm.jmp_near();
                
                // True path
                let true_pos = asm.current_offset();
                asm.mov_reg_imm64(Reg::Rax, 0x7FFB_0000_0000_0000u64);
                
                let end_pos = asm.current_offset();
                let disp_true = (true_pos as i32 - (patch_true as i32 + 4)) as u32;
                asm.patch_u32(patch_true, disp_true);
                let disp_end = (end_pos as i32 - (patch_end as i32 + 4)) as u32;
                asm.patch_u32(patch_end, disp_end);
            }
            _ => {} // Generic/VmValue is already boxed in Rax
        }

        emit_store(asm, Reg::Rax, dest, regmap);
        emit_reload_all_except(asm, regmap, Some(dest));
    } else {
        // --- SLOW PATH FALLBACK ---
        let fn_addr = slow_addr;
        let (helper_addr, second_arg) = if fn_addr != 0 {
            (helpers.jit_call_native_fnptr, fn_addr as u64)
        } else {
            (helpers.jit_call_native_op, op_id)
        };

        emit_flush_all(asm, regmap);

        asm.push(ARG_CTX);
        asm.push(ARG_EXEC_CTX);
        asm.push(ARG_BASE);

        let need_align = (regmap.used_phys.len() + 4) % 2 != 0;
        if need_align {
            asm.push(Reg::Rax);
        }

        // helper(ctx, fn_addr | op_id, args_start = base + dest, total)
        asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
        asm.mov_reg_imm64(ARG_CLOSURE, second_arg);
        asm.add_reg_imm32(ARG_BASE, dest as i32);
        asm.mov_reg_imm64(ARG_EXEC_CTX, total as u64);

        #[cfg(target_os = "windows")]
        asm.add_reg_imm8(Reg::Rsp, -32);

        asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
        asm.call_reg(Reg::R10);

        #[cfg(target_os = "windows")]
        asm.add_reg_imm8(Reg::Rsp, 32);

        if need_align {
            asm.pop(Reg::R11);
        }

        asm.pop(ARG_BASE);
        asm.pop(ARG_EXEC_CTX);
        asm.pop(ARG_CTX);

        asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

        asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
        asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
        asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

        emit_store(asm, Reg::Rax, dest, regmap);
        emit_reload_all_except(asm, regmap, Some(dest));
    }
}
