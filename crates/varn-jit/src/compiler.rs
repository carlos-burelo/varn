use varn_core::OpCode;
use varn_types::FunctionProto;

use crate::assembler::{Assembler, Reg, Cond};
use crate::mem::JitBuffer;
use crate::safepoint::{emit_load_reg, emit_store_reg};

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
                // Return has 1 extra word operand in bytecode
                ip += 1;
            }
            OpCode::LoadNull
            | OpCode::LoadTrue
            | OpCode::LoadFalse
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne => {
                // These have no extra operand words
            }
            OpCode::LoadInt => {
                // LoadInt has 1 extra word operand (immediate)
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
                // Move has 1 extra word operand
                ip += 1;
            }
            OpCode::AddImm
            | OpCode::SubImm => {
                // These have 1 extra word operand
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
                // These have 1 extra word operand (src1/src2)
                ip += 1;
            }
            OpCode::Jump
            | OpCode::Loop
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue => {
                // Jumps have 2 extra word operands for 32-bit big-endian offset
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

    // 2. Main Compilation Pass
    let mut asm = Assembler::new();
    let mut ip = 0;
    let code_len = code.len();

    // Map from bytecode offset to assembler instruction byte offset
    let mut ip_to_asm_pos = vec![0; code_len + 1];
    let mut jump_patches: Vec<JumpPatch> = Vec::new();

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
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadTrue => {
                let true_val = 0x7FFB_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, true_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadFalse => {
                let false_val = 0x7FFA_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, false_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadInt => {
                let val = code[ip] as i16;
                ip += 1;
                let int_val = 0x7FFC_0000_0000_0000u64 | (val as u64 & 0x0000_FFFF_FFFF_FFFFu64);
                asm.mov_reg_imm64(Reg::Rax, int_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadIntZero => {
                let int_val = 0x7FFC_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, int_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadIntOne => {
                let int_val = 0x7FFC_0000_0000_0001u64;
                asm.mov_reg_imm64(Reg::Rax, int_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadIntMinusOne => {
                let int_val = 0x7FFC_FFFF_FFFF_FFFFu64;
                asm.mov_reg_imm64(Reg::Rax, int_val);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadConst => {
                let idx = code[ip] as usize;
                ip += 1;

                // 1. Push/preserve registers
                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);
                asm.push(Reg::Rbx);

                // 2. Allocate shadow space on Windows
                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                // 3. Set up helper arguments
                asm.mov_reg_reg(ARG_CTX, ARG_CLOSURE); // Argument 1: closure
                asm.mov_reg_imm64(ARG_CLOSURE, idx as u64); // Argument 2: idx

                // 4. Call helper
                asm.mov_reg_imm64(Reg::R10, helpers.load_const as u64);
                asm.call_reg(Reg::R10);

                // 5. Deallocate shadow space on Windows
                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                // 6. Pop/restore registers
                asm.pop(Reg::Rbx);
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                // 7. Store return value (in Rax) back to VM stack in first_reg
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::LoadGlobalIdx => {
                let idx = code[ip] as usize;
                ip += 1;

                // 1. Push/preserve registers
                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);
                asm.push(Reg::Rbx);

                // 2. Allocate shadow space on Windows
                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                // 3. Set up helper arguments
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX); // Argument 1: exec_ctx
                asm.mov_reg_imm64(ARG_CLOSURE, idx as u64); // Argument 2: idx

                // 4. Call helper
                asm.mov_reg_imm64(Reg::R10, helpers.load_global_idx as u64);
                asm.call_reg(Reg::R10);

                // 5. Deallocate shadow space on Windows
                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                // 6. Pop/restore registers
                asm.pop(Reg::Rbx);
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                // 7. Store return value (in Rax) back to VM stack in first_reg
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;
                let idx = code[ip] as usize;
                ip += 1;

                // 1. Load src value into Rax
                emit_load_reg(&mut asm, Reg::Rax, src);

                // 2. Push/preserve registers
                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);
                asm.push(Reg::Rbx);

                // 3. Allocate shadow space on Windows
                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                // 4. Set up helper arguments
                asm.mov_reg_reg(ARG_BASE, Reg::Rax); // Argument 3: val
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX); // Argument 1: exec_ctx
                asm.mov_reg_imm64(ARG_CLOSURE, idx as u64); // Argument 2: idx

                // 5. Call helper
                let helper_addr = if op == OpCode::StoreGlobalIdx {
                    helpers.store_global_idx
                } else {
                    helpers.define_global_idx
                };
                asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
                asm.call_reg(Reg::R10);

                // 6. Deallocate shadow space on Windows
                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                // 7. Pop/restore registers
                asm.pop(Reg::Rbx);
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);
            }
            OpCode::Move => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;
                emit_load_reg(&mut asm, Reg::Rax, src);
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::Return => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 & 0xFF) as usize;
                emit_load_reg(&mut asm, Reg::Rax, src);
                asm.ret();
            }
            OpCode::AddImm | OpCode::SubImm => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;
                let imm = (w1 & 0xFF) as i8 as i32;

                // Load virtual register `src` into physical register `rax`
                emit_load_reg(&mut asm, Reg::Rax, src);

                // Sign-extend 48-bit to 64-bit signed integer
                asm.shl_reg_imm8(Reg::Rax, 16);
                asm.sar_reg_imm8(Reg::Rax, 16);

                // Perform the addition or subtraction of the immediate
                let adjust = if op == OpCode::SubImm {
                    -imm
                } else {
                    imm
                };
                asm.add_reg_imm32(Reg::Rax, adjust);

                // Mask back to 48-bit and apply TAG_INT tag (0x7FFC_0000_0000_0000)
                asm.mov_reg_imm64(Reg::R10, 0x0000_FFFF_FFFF_FFFFu64);
                asm.and_reg_reg(Reg::Rax, Reg::R10);
                asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64);
                asm.or_reg_reg(Reg::Rax, Reg::R10);

                // Store back to the VM stack
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
            }
            OpCode::AddInt | OpCode::SubInt | OpCode::MulInt => {
                let w1 = code[ip];
                ip += 1;
                let src1 = (w1 >> 8) as usize;
                let src2 = (w1 & 0xFF) as usize;

                // Load virtual registers into physical registers
                emit_load_reg(&mut asm, Reg::Rax, src1);
                emit_load_reg(&mut asm, Reg::R9, src2);

                // Sign-extend 48-bit to 64-bit signed integer
                asm.shl_reg_imm8(Reg::Rax, 16);
                asm.sar_reg_imm8(Reg::Rax, 16);
                asm.shl_reg_imm8(Reg::R9, 16);
                asm.sar_reg_imm8(Reg::R9, 16);

                // Execute the native x86_64 instruction
                match op {
                    OpCode::AddInt => asm.add_reg_reg(Reg::Rax, Reg::R9),
                    OpCode::SubInt => asm.sub_reg_reg(Reg::Rax, Reg::R9),
                    OpCode::MulInt => asm.imul_reg_reg(Reg::Rax, Reg::R9),
                    _ => unreachable!(),
                }

                // Mask back to 48-bit and apply TAG_INT tag (0x7FFC_0000_0000_0000)
                asm.mov_reg_imm64(Reg::R10, 0x0000_FFFF_FFFF_FFFFu64);
                asm.and_reg_reg(Reg::Rax, Reg::R10);
                asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64);
                asm.or_reg_reg(Reg::Rax, Reg::R10);

                // Store back to the VM stack
                emit_store_reg(&mut asm, Reg::Rax, first_reg);
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

                emit_load_reg(&mut asm, Reg::Rax, src1);
                emit_load_reg(&mut asm, Reg::R9, src2);

                asm.shl_reg_imm8(Reg::Rax, 16);
                asm.sar_reg_imm8(Reg::Rax, 16);
                asm.shl_reg_imm8(Reg::R9, 16);
                asm.sar_reg_imm8(Reg::R9, 16);

                asm.cmp_reg_reg(Reg::Rax, Reg::R9);

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

                // False path
                let false_val = 0x7FFA_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, false_val);
                let jmp_end_patch = asm.jmp_near();

                // True path
                let true_pos = asm.current_offset();
                let disp_true = (true_pos as i32 - (jmp_true_patch as i32 + 4)) as u32;
                asm.patch_u32(jmp_true_patch, disp_true);

                let true_val = 0x7FFB_0000_0000_0000u64;
                asm.mov_reg_imm64(Reg::Rax, true_val);

                // End path
                let end_pos = asm.current_offset();
                let disp_end = (end_pos as i32 - (jmp_end_patch as i32 + 4)) as u32;
                asm.patch_u32(jmp_end_patch, disp_end);

                emit_store_reg(&mut asm, Reg::Rax, first_reg);
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

                emit_load_reg(&mut asm, Reg::Rax, first_reg);

                // Compare RAX with false, null, and int_zero
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

                emit_load_reg(&mut asm, Reg::Rax, first_reg);

                // If falsy, jump past the unconditional jump to target
                asm.mov_reg_imm64(Reg::R10, 0x7FFA_0000_0000_0000u64); // bool_false
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p_false = asm.jmp_cond(Cond::Equal);

                asm.mov_reg_imm64(Reg::R10, 0x7FF9_0000_0000_0000u64); // null
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p_null = asm.jmp_cond(Cond::Equal);

                asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64); // int_zero
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p_zero = asm.jmp_cond(Cond::Equal);

                // If not falsy, then it is truthy, so jump to target
                let p_target = asm.jmp_near();
                jump_patches.push(JumpPatch { patch_pos: p_target, target_bytecode_ip });

                // Falsy label
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
