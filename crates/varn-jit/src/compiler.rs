use varn_core::OpCode;
use varn_types::FunctionProto;

use crate::assembler::{Assembler, Reg, Cond};
use crate::mem::JitBuffer;
use crate::safepoint::emit_load_reg;
use crate::regalloc::{RegMap, emit_prologue, emit_epilogue, emit_load, emit_store, emit_flush_all, emit_reload_all, emit_reload_all_except};

use crate::registers::{ARG_CTX, ARG_CLOSURE, ARG_BASE, ARG_EXEC_CTX};

struct JumpPatch {
    patch_pos: usize,
    target_bytecode_ip: usize,
}

/// Compiles a `FunctionProto` bytecode function into native x86_64 machine code.
/// Returns a `JitBuffer` containing the executable pages on success, or a bailout error on failure.
pub fn compile_proto(proto: &FunctionProto, helpers: crate::JitHelpers) -> Result<JitBuffer, String> {
    // 1. Audit opcodes to verify if we support all of them.
    // If we find any unsupported opcode, we perform an eager bailout to the interpreter.
    let code = &proto.chunk.code;
    let mut ip = 0;
    while ip < code.len() {
        let raw_op = code[ip];
        ip += 1;
        let op = OpCode::from_u8(raw_op as u8)
            .ok_or_else(|| format!("Unknown opcode: {}", raw_op))?;

        match op {
            OpCode::Return => {
                ip += 1;
            }
            OpCode::LoadNull
            | OpCode::LoadTrue
            | OpCode::LoadFalse
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne => {}
            OpCode::LoadInt => {
                ip += 1;
            }
            OpCode::LoadConst
            | OpCode::LoadGlobalIdx => {
                ip += 1;
            }
            OpCode::StoreGlobalIdx
            | OpCode::DefineGlobalIdx => {
                ip += 2;
            }
            OpCode::Move => {
                ip += 1;
            }
            OpCode::AddImm
            | OpCode::SubImm => {
                ip += 1;
            }
            OpCode::AddInt
            | OpCode::SubInt
            | OpCode::MulInt
            | OpCode::LtInt
            | OpCode::GtInt
            | OpCode::LteInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt => {
                ip += 1;
            }
            OpCode::Jump
            | OpCode::Loop
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue => {
                ip += 2;
            }
            _ => {
                return Err(format!(
                    "JIT Bailout: Opcode '{:?}' is not supported in the JIT compiler",
                    op
                ));
            }
        }
    }

    // 1b. Build register allocation map from bytecode frequency analysis.
    let regmap = RegMap::from_bytecode(code);

    // 2. Main Compilation Pass
    let mut asm = Assembler::new();
    let mut ip = 0;
    let code_len = code.len();

    // Map from bytecode offset to assembler instruction byte offset
    let mut ip_to_asm_pos = vec![0; code_len + 1];
    let mut jump_patches: Vec<JumpPatch> = Vec::new();

    // Emit function prologue: save callee-saved physical regs.
    // We also load the initial values of all allocated virtual regs from the VM stack.
    emit_prologue(&mut asm, &regmap);
    // Load initial values from VM stack into allocated physical regs
    for (&vreg, &phys) in regmap.map_iter() {
        emit_load_reg(&mut asm, phys, vreg);
    }

    while ip < code_len {
        ip_to_asm_pos[ip] = asm.current_offset();

        let raw_op = code[ip];
        ip += 1;
        let first_reg = (raw_op >> 8) as usize;
        let op = OpCode::from_u8(raw_op as u8).unwrap();

        match op {
            OpCode::LoadNull => {
                let null_val = 0x7FF9_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, null_val);
                emit_store(&mut asm, Reg::Rax, first_reg, &regmap);
            }
            OpCode::LoadTrue => {
                let true_val = 0x7FFB_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, true_val);
                emit_store(&mut asm, Reg::Rax, first_reg, &regmap);
            }
            OpCode::LoadFalse => {
                let false_val = 0x7FFA_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, false_val);
                emit_store(&mut asm, Reg::Rax, first_reg, &regmap);
            }
            OpCode::LoadInt => {
                let val = code[ip] as i16;
                ip += 1;
                let int_val = 0x7FFC_0000_0000_0000u64 | (val as u64 & 0x0000_FFFF_FFFF_FFFFu64);
                asm.mov_reg_imm64(Reg::Rax, int_val);
                emit_store(&mut asm, Reg::Rax, first_reg, &regmap);
            }
            OpCode::LoadIntZero => {
                let int_val = 0x7FFC_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, int_val);
                emit_store(&mut asm, Reg::Rax, first_reg, &regmap);
            }
            OpCode::LoadIntOne => {
                let int_val = 0x7FFC_0000_0000_0001u64;
                asm.mov_reg_imm64(Reg::Rax, int_val);
                emit_store(&mut asm, Reg::Rax, first_reg, &regmap);
            }
            OpCode::LoadIntMinusOne => {
                let int_val = 0x7FFC_FFFF_FFFF_FFFFu64;
                asm.mov_reg_imm64(Reg::Rax, int_val);
                emit_store(&mut asm, Reg::Rax, first_reg, &regmap);
            }
            OpCode::LoadConst => {
                let idx = code[ip] as usize;
                ip += 1;

                // Flush allocated regs to memory before helper call
                emit_flush_all(&mut asm, &regmap);

                // Push/preserve arg regs
                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                // Align stack to 16 bytes:
                // Total bytes pushed = 8 (ret) + 8 (Rbp) + K*8 (used_phys) + 32 (args) = K*8 + 48.
                // 48 is a multiple of 16. If K is odd, we need 8 bytes padding, so we push Rax as dummy.
                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_CTX, ARG_CLOSURE);
                asm.mov_reg_imm64(ARG_CLOSURE, idx as u64);

                asm.mov_reg_imm64(Reg::R10, helpers.load_const as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                // Save result before pops clobber Rax
                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);

                // Reload allocated regs from memory after helper call
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::LoadGlobalIdx => {
                let idx = code[ip] as usize;
                ip += 1;

                // Flush allocated regs to memory before helper call
                emit_flush_all(&mut asm, &regmap);

                 asm.push(ARG_CTX);
                 asm.push(ARG_CLOSURE);
                 asm.push(ARG_BASE);
                 asm.push(ARG_EXEC_CTX);

                 // Align stack to 16 bytes:
                 // Total bytes pushed = 8 (ret) + 8 (Rbp) + K*8 (used_phys) + 32 (args) = K*8 + 48.
                 // 48 is a multiple of 16. If K is odd, we need 8 bytes padding, so we push Rax as dummy.
                 let need_dummy = regmap.used_phys.len() % 2 != 0;
                 if need_dummy {
                     asm.push(Reg::Rax);
                 }

                 #[cfg(target_os = "windows")]
                 asm.add_reg_imm8(Reg::Rsp, -32);

                 asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
                 asm.mov_reg_imm64(ARG_CLOSURE, idx as u64);

                 asm.mov_reg_imm64(Reg::R10, helpers.load_global_idx as u64);
                 asm.call_reg(Reg::R10);

                 #[cfg(target_os = "windows")]
                 asm.add_reg_imm8(Reg::Rsp, 32);

                 // Save result before pops clobber Rax
                 asm.mov_reg_reg(Reg::R11, Reg::Rax);

                 if need_dummy {
                     asm.pop(Reg::Rax);
                 }
                 asm.pop(ARG_EXEC_CTX);
                 asm.pop(ARG_BASE);
                 asm.pop(ARG_CLOSURE);
                 asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);

                // Reload allocated regs from memory after helper call
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;
                let idx = code[ip] as usize;
                ip += 1;

                // Load src value — may be in a physical reg
                emit_load(&mut asm, Reg::Rax, src, &regmap);

                // Flush allocated regs to memory before helper call
                emit_flush_all(&mut asm, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                // Align stack to 16 bytes:
                // Total bytes pushed = 8 (ret) + 8 (Rbp) + K*8 (used_phys) + 32 (args) = K*8 + 48.
                // 48 is a multiple of 16. If K is odd, we need 8 bytes padding, so we push Rax as dummy.
                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                // Rax holds the value to store. Need it as arg 3.
                // On Windows: arg1=RCX, arg2=RDX, arg3=R8. ARG_BASE=R8.
                // On Unix: arg1=RDI, arg2=RSI, arg3=RDX. ARG_BASE=RDX.
                // Save value from Rax to R11 before we clobber arg regs.
                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_BASE, Reg::R11);   // arg3: val
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX); // arg1: exec_ctx
                asm.mov_reg_imm64(ARG_CLOSURE, idx as u64); // arg2: idx

                let helper_addr = if op == OpCode::StoreGlobalIdx {
                    helpers.store_global_idx
                } else {
                    helpers.define_global_idx
                };
                asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                // Reload allocated regs from memory after helper call
                emit_reload_all(&mut asm, &regmap);
            }
            OpCode::Move => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;
                emit_load(&mut asm, Reg::Rax, src, &regmap);
                emit_store(&mut asm, Reg::Rax, first_reg, &regmap);
            }
            OpCode::Return => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 & 0xFF) as usize;
                emit_load(&mut asm, Reg::Rax, src, &regmap);
                // Save return value before epilogue pops clobber regs
                asm.mov_reg_reg(Reg::R11, Reg::Rax);
                emit_epilogue(&mut asm, &regmap);
                asm.mov_reg_reg(Reg::Rax, Reg::R11);
                asm.ret();
            }
            OpCode::AddImm | OpCode::SubImm => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;
                let imm = (w1 & 0xFF) as i8 as i32;

                emit_load(&mut asm, Reg::Rax, src, &regmap);

                asm.shl_reg_imm8(Reg::Rax, 16);
                asm.sar_reg_imm8(Reg::Rax, 16);

                let adjust = if op == OpCode::SubImm { -imm } else { imm };
                asm.add_reg_imm32(Reg::Rax, adjust);

                asm.mov_reg_imm64(Reg::R10, 0x0000_FFFF_FFFF_FFFFu64);
                asm.and_reg_reg(Reg::Rax, Reg::R10);
                asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64);
                asm.or_reg_reg(Reg::Rax, Reg::R10);

                emit_store(&mut asm, Reg::Rax, first_reg, &regmap);
            }
            OpCode::AddInt | OpCode::SubInt | OpCode::MulInt => {
                let w1 = code[ip];
                ip += 1;
                let src1 = (w1 >> 8) as usize;
                let src2 = (w1 & 0xFF) as usize;

                emit_load(&mut asm, Reg::Rax, src1, &regmap);
                emit_load(&mut asm, Reg::R11, src2, &regmap);

                asm.shl_reg_imm8(Reg::Rax, 16);
                asm.sar_reg_imm8(Reg::Rax, 16);
                asm.shl_reg_imm8(Reg::R11, 16);
                asm.sar_reg_imm8(Reg::R11, 16);

                match op {
                    OpCode::AddInt => asm.add_reg_reg(Reg::Rax, Reg::R11),
                    OpCode::SubInt => asm.sub_reg_reg(Reg::Rax, Reg::R11),
                    OpCode::MulInt => asm.imul_reg_reg(Reg::Rax, Reg::R11),
                    _ => unreachable!(),
                }

                asm.mov_reg_imm64(Reg::R10, 0x0000_FFFF_FFFF_FFFFu64);
                asm.and_reg_reg(Reg::Rax, Reg::R10);
                asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64);
                asm.or_reg_reg(Reg::Rax, Reg::R10);

                emit_store(&mut asm, Reg::Rax, first_reg, &regmap);
            }
            OpCode::LtInt
            | OpCode::GtInt
            | OpCode::LteInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt => {
                let w1 = code[ip];
                ip += 1;
                let src1 = (w1 >> 8) as usize;
                let src2 = (w1 & 0xFF) as usize;

                emit_load(&mut asm, Reg::Rax, src1, &regmap);
                emit_load(&mut asm, Reg::R11, src2, &regmap);

                asm.shl_reg_imm8(Reg::Rax, 16);
                asm.sar_reg_imm8(Reg::Rax, 16);
                asm.shl_reg_imm8(Reg::R11, 16);
                asm.sar_reg_imm8(Reg::R11, 16);

                asm.cmp_reg_reg(Reg::Rax, Reg::R11);

                let cond = match op {
                    OpCode::LtInt => Cond::Less,
                    OpCode::GtInt => Cond::Greater,
                    OpCode::LteInt => Cond::LessEqual,
                    OpCode::GteInt => Cond::GreaterEqual,
                    OpCode::EqInt => Cond::Equal,
                    OpCode::NeqInt => Cond::NotEqual,
                    _ => unreachable!(),
                };

                let jmp_true_patch = asm.jmp_cond(cond);

                let false_val = 0x7FFA_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, false_val);
                let jmp_end_patch = asm.jmp_near();

                let true_pos = asm.current_offset();
                let disp_true = (true_pos as i32 - (jmp_true_patch as i32 + 4)) as u32;
                asm.patch_u32(jmp_true_patch, disp_true);

                let true_val = 0x7FFB_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, true_val);

                let end_pos = asm.current_offset();
                let disp_end = (end_pos as i32 - (jmp_end_patch as i32 + 4)) as u32;
                asm.patch_u32(jmp_end_patch, disp_end);

                emit_store(&mut asm, Reg::Rax, first_reg, &regmap);
            }
            OpCode::Jump => {
                let offset = ((code[ip] as u32) << 16 | code[ip + 1] as u32) as usize;
                let target_bytecode_ip = ip + 2 + offset;
                ip += 2;

                let patch_pos = asm.jmp_near();
                jump_patches.push(JumpPatch {
                    patch_pos,
                    target_bytecode_ip,
                });
            }
            OpCode::Loop => {
                let offset = ((code[ip] as u32) << 16 | code[ip + 1] as u32) as usize;
                let target_bytecode_ip = ip + 2 - offset;
                ip += 2;

                let patch_pos = asm.jmp_near();
                jump_patches.push(JumpPatch {
                    patch_pos,
                    target_bytecode_ip,
                });
            }
            OpCode::JumpIfFalse => {
                let offset = ((code[ip] as u32) << 16 | code[ip + 1] as u32) as usize;
                let target_bytecode_ip = ip + 2 + offset;
                ip += 2;

                emit_load(&mut asm, Reg::Rax, first_reg, &regmap);

                asm.mov_reg_imm64(Reg::R10, 0x7FFA_0000_0000_0000u64); // bool_false
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p1 = asm.jmp_cond(Cond::Equal);

                asm.mov_reg_imm64(Reg::R10, 0x7FF9_0000_0000_0000u64); // null
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p2 = asm.jmp_cond(Cond::Equal);

                asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64); // int_zero
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p3 = asm.jmp_cond(Cond::Equal);

                jump_patches.push(JumpPatch { patch_pos: p1, target_bytecode_ip });
                jump_patches.push(JumpPatch { patch_pos: p2, target_bytecode_ip });
                jump_patches.push(JumpPatch { patch_pos: p3, target_bytecode_ip });
            }
            OpCode::JumpIfTrue => {
                let offset = ((code[ip] as u32) << 16 | code[ip + 1] as u32) as usize;
                let target_bytecode_ip = ip + 2 + offset;
                ip += 2;

                emit_load(&mut asm, Reg::Rax, first_reg, &regmap);

                asm.mov_reg_imm64(Reg::R10, 0x7FFA_0000_0000_0000u64); // bool_false
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p_false = asm.jmp_cond(Cond::Equal);

                asm.mov_reg_imm64(Reg::R10, 0x7FF9_0000_0000_0000u64); // null
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p_null = asm.jmp_cond(Cond::Equal);

                asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64); // int_zero
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p_zero = asm.jmp_cond(Cond::Equal);

                let p_target = asm.jmp_near();
                jump_patches.push(JumpPatch { patch_pos: p_target, target_bytecode_ip });

                let falsy_pos = asm.current_offset();
                let disp_false = (falsy_pos as i32 - (p_false as i32 + 4)) as u32;
                asm.patch_u32(p_false, disp_false);

                let disp_null = (falsy_pos as i32 - (p_null as i32 + 4)) as u32;
                asm.patch_u32(p_null, disp_null);

                let disp_zero = (falsy_pos as i32 - (p_zero as i32 + 4)) as u32;
                asm.patch_u32(p_zero, disp_zero);
            }
            _ => unreachable!(),
        }
    }

    ip_to_asm_pos[code_len] = asm.current_offset();

    // 3. Post-compilation Jump Patching Pass
    for patch in &jump_patches {
        let target_asm_pos = ip_to_asm_pos[patch.target_bytecode_ip];
        let displacement = (target_asm_pos as isize - (patch.patch_pos + 4) as isize) as i32;
        asm.patch_u32(patch.patch_pos, displacement as u32);
    }

    // 4. Assemble and mark executable
    let native_bytes = asm.into_bytes();
    let mut jit_buf = JitBuffer::new(native_bytes.len())?;
    jit_buf.as_mut_slice()[..native_bytes.len()].copy_from_slice(&native_bytes);
    jit_buf.make_executable()?;

    Ok(jit_buf)
}
