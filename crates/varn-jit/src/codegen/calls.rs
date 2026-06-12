use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_calls(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) -> Result<(), String> {
    match op {
        OpCode::Call => emit_call(ctx, first_reg),
        OpCode::CallSelf => emit_call_self(ctx, first_reg),
        OpCode::CallMethod => emit_call_method(ctx, first_reg),
        OpCode::InvokeVirtual => emit_invoke_virtual(ctx),
        OpCode::Intrinsic => emit_intrinsic(ctx, first_reg),
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

    // 1. Flush caller registers to stack slots in caller so callee can read arguments
    emit_flush_all(asm, regmap);

    // 2. Load callee VmValue into Rax
    emit_load(asm, Reg::Rax, callee_reg, regmap);

    // 3. Save caller JIT argument/context registers
    let need_prepare_dummy = regmap.used_phys.len() % 2 != 0;
    if need_prepare_dummy {
        asm.push(Reg::Rax);
    }
    asm.push(ARG_CTX);
    asm.push(ARG_EXEC_CTX);
    asm.push(ARG_BASE);

    // 4. Prepare arguments for helper: jit_prepare_call(ctx, callee, callee_base)
    // Rcx/Rdi = ctx
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    // Rdx/Rsi = callee
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    // R8/Rdx = callee_base (ARG_BASE + arg_start)
    asm.add_reg_imm32(ARG_BASE, arg_start as i32);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.jit_prepare_call as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    // 5. Check if returned closure pointer in Rax is null
    asm.cmp_reg_imm32(Reg::Rax, 0);
    let fallback_patch = asm.jmp_cond(Cond::Equal);

    // ==========================================
    // FAST PATH: Direct JIT call
    // ==========================================
    // Pop the registers saved for jit_prepare_call
    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);
    if need_prepare_dummy {
        asm.pop(Reg::R11);
    }

    // Reload ARG_CTX from ExecCtx.stack (offset 8 of ARG_EXEC_CTX) in case stack reallocated
    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    // Recompute REG_FRAME_BASE = ARG_CTX + ARG_BASE * 8
    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    // Save caller JIT registers across the JIT-to-JIT call
    asm.push(ARG_CTX);
    asm.push(ARG_EXEC_CTX);
    asm.push(ARG_BASE);

    let need_align = regmap.used_phys.len() % 2 != 0;
    if need_align {
        asm.push(Reg::Rax);
    }

    // Compute callee_base into R11: ARG_BASE + arg_start
    asm.mov_reg_reg(Reg::R11, ARG_BASE);
    asm.add_reg_imm32(Reg::R11, arg_start as i32);

    // Set up arguments for JIT function call:
    // Rcx/Rdi (ARG_CTX): unchanged (values array pointer)
    // Rdx/Rsi (ARG_CLOSURE): closure pointer (returned in Rax)
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    // R8/Rdx (ARG_BASE): callee_base (R11)
    asm.mov_reg_reg(ARG_BASE, Reg::R11);
    // R9/Rcx (ARG_EXEC_CTX): unchanged (ExecCtx pointer)

    // Load JIT entry point from closure (offset 56) into R10
    asm.mov_reg_mem(Reg::R10, ARG_CLOSURE, 56);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    // Execute direct JIT call!
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    if need_align {
        asm.pop(Reg::R11);
    }

    // Restore caller JIT registers
    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);

    // Save ARG_BASE and ARG_EXEC_CTX across the helper call
    let need_dummy = regmap.used_phys.len() % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    // Call jit_post_call(ctx, callee_base, val):
    // 1st arg: ctx (ARG_CTX = ARG_EXEC_CTX)
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    // 2nd arg: callee_base (ARG_CLOSURE = ARG_BASE + arg_start)
    asm.mov_reg_reg(ARG_CLOSURE, ARG_BASE);
    asm.add_reg_imm32(ARG_CLOSURE, arg_start as i32);
    // 3rd arg: val (ARG_BASE = Reg::Rax)
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

    // Reload ARG_CTX from ExecCtx.stack.ptr (offset 8) in case of stack reallocation
    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    // Recompute REG_FRAME_BASE = ARG_CTX + ARG_BASE * 8
    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::Rax, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
    let end_patch = asm.jmp_near();

    // ==========================================
    // FALLBACK PATH: Slow/general call dispatch
    // ==========================================
    let fallback_pos = asm.current_offset();
    let relative_fallback = (fallback_pos as i64 - (fallback_patch as i64 + 4)) as i32;
    asm.patch_u32(fallback_patch, relative_fallback as u32);

    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);
    if need_prepare_dummy {
        asm.pop(Reg::R11);
    }

    // Reload ARG_CTX from ExecCtx.stack (offset 8 of ARG_EXEC_CTX) in case stack reallocated
    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    // Recompute REG_FRAME_BASE = ARG_CTX + ARG_BASE * 8
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

    // Reload ARG_CTX from ExecCtx.stack.ptr (offset 8) in case stack reallocated
    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    // Recompute REG_FRAME_BASE = ARG_CTX + ARG_BASE * 8
    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::R11, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));

    // End patch
    let end_pos = asm.current_offset();
    let relative_end = (end_pos as i64 - (end_patch as i64 + 4)) as i32;
    asm.patch_u32(end_patch, relative_end as u32);
}

fn emit_call_self(ctx: &mut CodegenCtx, first_reg: usize) {
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
    let callee_dummy = (w1 & 0xFF) as usize;
    let arg_count = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    let _ = first_reg;
    let _ = callee_dummy;
    let _ = arg_count;

    // 1. Flush caller registers to stack slots in caller so callee can read arguments
    emit_flush_all(asm, regmap);

    // 2. Save caller JIT registers across the frame push helper call
    let need_dummy_push = regmap.used_phys.len() % 2 != 0;
    if need_dummy_push {
        asm.push(Reg::Rax);
    }
    asm.push(ARG_CTX);
    asm.push(ARG_EXEC_CTX);
    asm.push(ARG_BASE);

    // Call jit_push_self_frame(ctx, callee_base)
    // 1st arg: ctx (ARG_CTX = ARG_EXEC_CTX)
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    // 2nd arg: callee_base (ARG_CLOSURE = ARG_BASE + arg_start)
    asm.mov_reg_reg(ARG_CLOSURE, ARG_BASE);
    asm.add_reg_imm32(ARG_CLOSURE, arg_start as i32);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.jit_push_self_frame as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    // Restore caller JIT registers
    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);
    if need_dummy_push {
        asm.pop(Reg::Rax);
    }

    // jit_push_self_frame may have reallocated the VM stack — reload the stack data pointer.
    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    // Recompute REG_FRAME_BASE = ARG_CTX + ARG_BASE * 8 with fresh pointer
    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    // Reload closure pointer from prologue save slot at [rsp+8]
    asm.mov_reg_mem(ARG_CLOSURE, Reg::Rsp, 8);

    // 3. Save caller JIT registers across the JIT-to-JIT call
    asm.push(ARG_CTX);
    asm.push(ARG_EXEC_CTX);
    asm.push(ARG_BASE);

    let need_align = regmap.used_phys.len() % 2 != 0;
    if need_align {
        asm.push(Reg::Rax);
    }

    // Compute callee_base into R11: ARG_BASE + arg_start
    asm.mov_reg_reg(Reg::R11, ARG_BASE);
    asm.add_reg_imm32(Reg::R11, arg_start as i32);

    // Set up arguments for JIT function call:
    // Rcx/Rdi (ARG_CTX): unchanged (values array pointer)
    // Rdx/Rsi (ARG_CLOSURE): reloaded closure pointer
    // R8/Rdx (ARG_BASE): callee_base (R11)
    asm.mov_reg_reg(ARG_BASE, Reg::R11);
    // R9/Rcx (ARG_EXEC_CTX): unchanged (ExecCtx pointer)

    // Execute direct JIT call to the start of this function! (offset 0)
    let call_patch = asm.call_near();
    let displacement = (0 as isize - (call_patch + 4) as isize) as i32;
    asm.patch_u32(call_patch, displacement as u32);

    if need_align {
        asm.pop(Reg::R11);
    }

    // Restore caller JIT registers
    asm.pop(ARG_BASE);
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_CTX);

    // Save ARG_BASE and ARG_EXEC_CTX across the helper call
    let need_dummy = regmap.used_phys.len() % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    // Call jit_post_call(ctx, callee_base, val):
    // 1st arg: ctx (ARG_CTX = ARG_EXEC_CTX)
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    // 2nd arg: callee_base (ARG_CLOSURE = ARG_BASE + arg_start)
    asm.mov_reg_reg(ARG_CLOSURE, ARG_BASE);
    asm.add_reg_imm32(ARG_CLOSURE, arg_start as i32);
    // 3rd arg: val (ARG_BASE = Reg::Rax)
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

    // Reload ARG_CTX from ExecCtx.stack.ptr (offset 8) in case stack reallocated
    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    // Recompute REG_FRAME_BASE = ARG_CTX + ARG_BASE * 8
    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::Rax, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
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

    // Reload closure pointer from saved stack slot
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

    // Reload ARG_CTX from ExecCtx.stack.ptr (offset 8) in case stack reallocated
    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    // Recompute REG_FRAME_BASE = ARG_CTX + ARG_BASE * 8
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

    // Reload closure pointer from saved stack slot
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

    // Reload ARG_CTX from ExecCtx.stack.ptr (offset 8) in case stack reallocated
    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    // Recompute REG_FRAME_BASE = ARG_CTX + ARG_BASE * 8
    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::R11, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
}

/// Emit JIT code for `OpCode::Intrinsic`.
/// Bytecode layout: [Intrinsic | dest] [wire_byte | arg_count]
/// Calls `jit_dispatch_intrinsic(exec_ctx, wire_byte, args_start, arg_count) -> VmValue`.
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
    // dest is also the start of the args slice in the stack (object = arg[0])

    // Flush all live registers so the helper can read args from the stack
    emit_flush_all(asm, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_EXEC_CTX);
    asm.push(ARG_BASE);

    let need_align = (regmap.used_phys.len() + 4) % 2 != 0;
    if need_align {
        asm.push(Reg::Rax);
    }

    // Set up 4 arguments for jit_dispatch_intrinsic:
    // 1st (ARG_CTX)    = ExecCtx ptr  (currently in ARG_EXEC_CTX)
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    // 2nd (ARG_CLOSURE) = wire_byte
    asm.mov_reg_imm64(ARG_CLOSURE, wire_byte as u64);
    // 3rd (ARG_BASE)   = absolute stack index of args = ARG_BASE + dest
    asm.add_reg_imm32(ARG_BASE, dest as i32);
    // 4th (ARG_EXEC_CTX) = arg_count
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

    // Reload ARG_CTX (stack pointer) from ExecCtx in case heap reallocated
    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    // Recompute REG_FRAME_BASE = ARG_CTX + ARG_BASE * 8
    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    // Result is in Rax; store to dest register
    emit_store(asm, Reg::Rax, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
}
