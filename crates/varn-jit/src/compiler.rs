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
            | OpCode::LoadGlobalIdx
            | OpCode::LoadGlobal
            | OpCode::LoadUpvalue
            | OpCode::StoreUpvalue => {
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
            OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::EqFloat
            | OpCode::NeqFloat
            | OpCode::LtFloat
            | OpCode::LteFloat
            | OpCode::GtFloat
            | OpCode::GteFloat
            | OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::ToString
            | OpCode::IsNull
            | OpCode::Not
            | OpCode::Negate
            | OpCode::ArrayLength
            | OpCode::ArrayPush
            | OpCode::ArrayPop
            | OpCode::ArrayExtend
            | OpCode::StrConcat
            | OpCode::StrSlice
            | OpCode::StrLength
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::Ushr => {
                ip += 1;
            }
            OpCode::GetIndex => {
                ip += 1;
            }
            OpCode::SetIndex => {
                ip += 1;
            }
            OpCode::Typeof => {
                ip += 1;
            }
            OpCode::Instanceof => {
                ip += 1;
            }
            OpCode::MakeClosure => {
                let w1 = code[ip];
                ip += 1;
                let uv_count = (w1 & 0xFF) as usize;
                ip += 1;
                ip += uv_count;
            }
            OpCode::AddInt
            | OpCode::SubInt
            | OpCode::MulInt
            | OpCode::DivInt
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
            OpCode::GetProperty
            | OpCode::SetProperty
            | OpCode::Call
            | OpCode::BuildArray => {
                ip += 2;
            }
            OpCode::CallMethod => {
                ip += 3;
            }
            OpCode::BuildStr => {
                let w1 = code[ip];
                let count = (w1 >> 8) as usize;
                ip += 1 + count;
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
            }
            OpCode::LoadGlobalIdx => {
                let idx = code[ip] as usize;
                ip += 1;

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
            }
            OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;
                let idx = code[ip] as usize;
                ip += 1;

                // Load src value — may be in a physical reg
                emit_load(&mut asm, Reg::Rax, src, &regmap);

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
            }
            OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::DivInt
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::Ushr => {
                let w1 = code[ip];
                ip += 1;
                let src1 = (w1 >> 8) as usize;
                let src2 = (w1 & 0xFF) as usize;

                // 1. Flush physical registers to memory before helper call
                emit_flush_all(&mut asm, &regmap);

                // 2. Preserve arg registers
                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                // 3. Align stack to 16 bytes:
                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                // 4. Load the two operands into Rax and R11
                emit_load(&mut asm, Reg::Rax, src1, &regmap);
                emit_load(&mut asm, Reg::R11, src2, &regmap);

                // 5. Windows ABI Shadow Space
                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                // 6. Set up the three arguments for the extern "C" function:
                asm.mov_reg_reg(ARG_BASE, Reg::R11);      // Arg 3 = b
                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);  // Arg 2 = a
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // Arg 1 = exec_ctx

                // 7. Get the corresponding helper address
                let helper_addr = match op {
                    OpCode::Add | OpCode::AddFloat => helpers.add,
                    OpCode::Sub | OpCode::SubFloat => helpers.sub,
                    OpCode::Mul | OpCode::MulFloat => helpers.mul,
                    OpCode::Div | OpCode::DivFloat | OpCode::DivInt => helpers.div,
                    OpCode::Mod => helpers.modulo,
                    OpCode::Pow => helpers.pow,
                    OpCode::BitAnd => helpers.bit_and,
                    OpCode::BitOr => helpers.bit_or,
                    OpCode::BitXor => helpers.bit_xor,
                    OpCode::Shl => helpers.shl,
                    OpCode::Shr => helpers.shr,
                    OpCode::Ushr => helpers.ushr,
                    _ => unreachable!(),
                };

                asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
                asm.call_reg(Reg::R10);

                // 8. Restore Windows ABI Shadow Space
                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                // 9. Save result from Rax into R11 before pops clobber Rax
                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                // 10. Pop arguments and dummy alignment
                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                // 11. Save the result back to virtual register first_reg
                emit_store(&mut asm, Reg::R11, first_reg, &regmap);

                // 12. Reload all other physical registers safely from memory
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::EqFloat
            | OpCode::NeqFloat
            | OpCode::LtFloat
            | OpCode::LteFloat
            | OpCode::GtFloat
            | OpCode::GteFloat => {
                let w1 = code[ip];
                ip += 1;
                let src1 = (w1 >> 8) as usize;
                let src2 = (w1 & 0xFF) as usize;

                // 1. Preserve arg registers
                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                // 2. Align stack to 16 bytes:
                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                // 3. Load the two operands into Rax and R11
                emit_load(&mut asm, Reg::Rax, src1, &regmap);
                emit_load(&mut asm, Reg::R11, src2, &regmap);

                // 4. Windows ABI Shadow Space
                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                // 5. Set up the three arguments for the extern "C" function:
                asm.mov_reg_reg(ARG_BASE, Reg::R11);      // Arg 3 = b
                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);  // Arg 2 = a
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // Arg 1 = exec_ctx

                // 6. Get the corresponding helper address
                let helper_addr = match op {
                    OpCode::Eq | OpCode::EqFloat => helpers.eq,
                    OpCode::Neq | OpCode::NeqFloat => helpers.neq,
                    OpCode::Lt | OpCode::LtFloat => helpers.lt,
                    OpCode::Lte | OpCode::LteFloat => helpers.lte,
                    OpCode::Gt | OpCode::GtFloat => helpers.gt,
                    OpCode::Gte | OpCode::GteFloat => helpers.gte,
                    _ => unreachable!(),
                };

                asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
                asm.call_reg(Reg::R10);

                // 7. Restore Windows ABI Shadow Space
                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                // 8. Save result from Rax into R11 before pops clobber Rax
                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                // 9. Pop arguments and dummy alignment
                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                // 10. Save the result back to virtual register first_reg
                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
            }
            OpCode::ToString => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;

                // 1. Flush physical registers to memory before helper call
                emit_flush_all(&mut asm, &regmap);

                // 2. Preserve arg registers
                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                // 3. Align stack to 16 bytes:
                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                // 4. Load the operand into Rax
                emit_load(&mut asm, Reg::Rax, src, &regmap);

                // 5. Windows ABI Shadow Space
                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                // 6. Set up the two arguments for the extern "C" function:
                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);  // Arg 2 = v
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // Arg 1 = exec_ctx

                // 7. Call the helper address
                let helper_addr = helpers.to_string;
                asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
                asm.call_reg(Reg::R10);

                // 8. Restore Windows ABI Shadow Space
                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                // 9. Save result from Rax into R11 before pops clobber Rax
                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                // 10. Pop arguments and dummy alignment
                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                // 11. Save the result back to virtual register first_reg
                emit_store(&mut asm, Reg::R11, first_reg, &regmap);

                // 12. Reload all other physical registers safely from memory
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::Not => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;

                // Load operand into Rax
                emit_load(&mut asm, Reg::Rax, src, &regmap);

                // Check if the value is a boolean: (v & 0x7FFE_0000_0000_0000) == 0x7FFA_0000_0000_0000
                asm.mov_reg_reg(Reg::R11, Reg::Rax);
                asm.mov_reg_imm64(Reg::R10, 0x7FFE_0000_0000_0000u64);
                asm.and_reg_reg(Reg::R11, Reg::R10);
                asm.mov_reg_imm64(Reg::R10, 0x7FFA_0000_0000_0000u64);
                asm.cmp_reg_reg(Reg::R11, Reg::R10);

                // Jump if equal to the boolean fast path
                let patch_je = asm.jmp_cond(Cond::Equal);

                // --- Slow Path: FFI helper call ---
                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                emit_flush_all(&mut asm, &regmap);
                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_CLOSURE, Reg::R11);  // Arg 2 = v
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // Arg 1 = exec_ctx

                let helper_addr = helpers.logical_not;
                asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
                let patch_jmp = asm.jmp_near();

                // --- Fast Path: Boolean toggle ---
                let bool_pos = asm.current_offset();
                let disp_je = (bool_pos as i32 - (patch_je as i32 + 4)) as u32;
                asm.patch_u32(patch_je, disp_je);

                // Toggle bit 48: v ^ 0x0001_0000_0000_0000
                asm.mov_reg_imm64(Reg::R10, 0x0001_0000_0000_0000u64);
                asm.xor_reg_reg(Reg::Rax, Reg::R10);
                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                // --- End Path ---
                let end_pos = asm.current_offset();
                let disp_jmp = (end_pos as i32 - (patch_jmp as i32 + 4)) as u32;
                asm.patch_u32(patch_jmp, disp_jmp);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
            }
            OpCode::Negate => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;

                // Load operand into Rax
                emit_load(&mut asm, Reg::Rax, src, &regmap);

                // Check if the value is an integer: (v & 0x7FFF_0000_0000_0000) == 0x7FFC_0000_0000_0000
                asm.mov_reg_reg(Reg::R11, Reg::Rax);
                asm.mov_reg_imm64(Reg::R10, 0x7FFF_0000_0000_0000u64);
                asm.and_reg_reg(Reg::R11, Reg::R10);
                asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64);
                asm.cmp_reg_reg(Reg::R11, Reg::R10);

                // Jump if equal to the integer fast path
                let patch_je = asm.jmp_cond(Cond::Equal);

                // --- Slow Path: FFI helper call ---
                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                emit_flush_all(&mut asm, &regmap);
                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_CLOSURE, Reg::R11);  // Arg 2 = v
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // Arg 1 = exec_ctx

                let helper_addr = helpers.negate;
                asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
                let patch_jmp = asm.jmp_near();

                // --- Fast Path: Integer Negation ---
                let int_pos = asm.current_offset();
                let disp_je = (int_pos as i32 - (patch_je as i32 + 4)) as u32;
                asm.patch_u32(patch_je, disp_je);

                // Shift left 16, arithmetic shift right 16 to sign extend
                asm.shl_reg_imm8(Reg::Rax, 16);
                asm.sar_reg_imm8(Reg::Rax, 16);

                // Negate: Rax = 0 - Rax
                asm.mov_reg_reg(Reg::R10, Reg::Rax);
                asm.mov_reg_imm64(Reg::Rax, 0);
                asm.sub_reg_reg(Reg::Rax, Reg::R10);

                // Re-tag: (rax & 0x0000_FFFF_FFFF_FFFF) | 0x7FFC_0000_0000_0000
                asm.mov_reg_imm64(Reg::R10, 0x0000_FFFF_FFFF_FFFFu64);
                asm.and_reg_reg(Reg::Rax, Reg::R10);
                asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64);
                asm.or_reg_reg(Reg::Rax, Reg::R10);
                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                // --- End Path ---
                let end_pos = asm.current_offset();
                let disp_jmp = (end_pos as i32 - (patch_jmp as i32 + 4)) as u32;
                asm.patch_u32(patch_jmp, disp_jmp);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
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
            OpCode::IsNull => {
                let src = (code[ip] >> 8) as usize;
                ip += 1;

                emit_load(&mut asm, Reg::Rax, src, &regmap);

                // Mask: (v & 0x7FFF_0000_0000_0000)
                asm.mov_reg_imm64(Reg::R11, 0x7FFF_0000_0000_0000u64);
                asm.and_reg_reg(Reg::Rax, Reg::R11);

                // Compare with 0x7FF9_0000_0000_0000u64
                asm.mov_reg_imm64(Reg::R11, 0x7FF9_0000_0000_0000u64);
                asm.cmp_reg_reg(Reg::Rax, Reg::R11);

                // Jump if equal to true_path
                let patch_je = asm.jmp_cond(Cond::Equal);

                // False path: load false_val (0x7FFA_0000_0000_0000u64)
                asm.mov_reg_imm64(Reg::Rax, 0x7FFA_0000_0000_0000u64);
                let patch_jmp = asm.jmp_near();

                // True path: load true_val (0x7FFB_0000_0000_0000u64)
                let true_pos = asm.current_offset();
                let disp_je = (true_pos as i32 - (patch_je as i32 + 4)) as u32;
                asm.patch_u32(patch_je, disp_je);

                asm.mov_reg_imm64(Reg::Rax, 0x7FFB_0000_0000_0000u64);

                // End path
                let end_pos = asm.current_offset();
                let disp_jmp = (end_pos as i32 - (patch_jmp as i32 + 4)) as u32;
                asm.patch_u32(patch_jmp, disp_jmp);

                // Store in first_reg
                emit_store(&mut asm, Reg::Rax, first_reg, &regmap);
            }
            OpCode::LoadGlobal => {
                let idx = code[ip] as usize;
                ip += 1;

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
                asm.mov_reg_imm64(ARG_BASE, idx as u64);

                asm.mov_reg_imm64(Reg::R10, helpers.load_global as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
            }
            OpCode::LoadUpvalue => {
                let w1 = code[ip];
                ip += 1;
                let dest = (w1 >> 8) as usize;
                let uv = (w1 & 0xFF) as usize;

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
                asm.mov_reg_imm64(ARG_BASE, uv as u64);

                asm.mov_reg_imm64(Reg::R10, helpers.load_upvalue as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, dest, &regmap);
            }
            OpCode::StoreUpvalue => {
                let w1 = code[ip];
                ip += 1;
                let uv = (w1 >> 8) as usize;
                let src = (w1 & 0xFF) as usize;

                emit_load(&mut asm, Reg::Rax, src, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
                asm.mov_reg_imm64(ARG_BASE, uv as u64);
                asm.mov_reg_reg(ARG_EXEC_CTX, Reg::Rax);

                asm.mov_reg_imm64(Reg::R10, helpers.store_upvalue as u64);
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
            }
            OpCode::MakeClosure => {
                let w1_ip = ip;
                let w1 = code[ip];
                ip += 1;
                let dest = (w1 >> 8) as usize;
                let uv_count = (w1 & 0xFF) as usize;
                ip += 1; // skip proto_idx
                ip += uv_count; // skip upvalues

                emit_flush_all(&mut asm, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
                asm.mov_reg_reg(ARG_EXEC_CTX, ARG_BASE);
                asm.mov_reg_imm64(ARG_BASE, w1_ip as u64);

                asm.mov_reg_imm64(Reg::R10, helpers.make_closure as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, dest, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(dest));
            }
            OpCode::GetProperty => {
                let w1 = code[ip];
                ip += 1;
                let obj_reg = (w1 >> 8) as usize;
                let cs_idx = (w1 & 0xFF) as usize;
                let name_idx = code[ip] as usize;
                ip += 1;

                emit_flush_all(&mut asm, &regmap);

                emit_load(&mut asm, Reg::Rax, obj_reg, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = (regmap.used_phys.len() + 5) % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                // Push JitGetPropertyArgs struct fields in reverse order:
                // 1. ip
                asm.mov_reg_imm64(Reg::R11, ip as u64);
                asm.push(Reg::R11);
                // 2. dest
                asm.mov_reg_imm64(Reg::R11, first_reg as u64);
                asm.push(Reg::R11);
                // 3. cs_idx
                asm.mov_reg_imm64(Reg::R11, cs_idx as u64);
                asm.push(Reg::R11);
                // 4. name_idx
                asm.mov_reg_imm64(Reg::R11, name_idx as u64);
                asm.push(Reg::R11);
                // 5. obj
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

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::SetProperty => {
                let w1 = code[ip];
                ip += 1;
                let val_reg = (w1 >> 8) as usize;
                let cs_idx = (w1 & 0xFF) as usize;
                let name_idx = code[ip] as usize;
                ip += 1;
                let obj_reg = first_reg;

                emit_flush_all(&mut asm, &regmap);

                emit_load(&mut asm, Reg::Rax, obj_reg, &regmap);
                emit_load(&mut asm, Reg::R11, val_reg, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = (regmap.used_phys.len() + 5) % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                // Push JitSetPropertyArgs struct fields in reverse order:
                // 1. ip
                asm.mov_reg_imm64(Reg::R10, ip as u64);
                asm.push(Reg::R10);
                // 2. cs_idx
                asm.mov_reg_imm64(Reg::R10, cs_idx as u64);
                asm.push(Reg::R10);
                // 3. name_idx
                asm.mov_reg_imm64(Reg::R10, name_idx as u64);
                asm.push(Reg::R10);
                // 4. val
                asm.push(Reg::R11);
                // 5. obj
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

                emit_reload_all(&mut asm, &regmap);
            }
            OpCode::Call => {
                let w1 = code[ip];
                ip += 1;
                let w2 = code[ip];
                ip += 1;
                let dest = (w1 >> 8) as usize;
                let callee_reg = (w1 & 0xFF) as usize;
                let arg_count = (w2 >> 8) as usize;
                let arg_start = (w2 & 0xFF) as usize;

                emit_flush_all(&mut asm, &regmap);

                emit_load(&mut asm, Reg::Rax, callee_reg, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = (regmap.used_phys.len() + 5) % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                // Push JitCallArgs struct fields in reverse order:
                // 1. ip
                asm.mov_reg_imm64(Reg::R11, ip as u64);
                asm.push(Reg::R11);
                // 2. dest
                asm.mov_reg_imm64(Reg::R11, dest as u64);
                asm.push(Reg::R11);
                // 3. arg_count
                asm.mov_reg_imm64(Reg::R11, arg_count as u64);
                asm.push(Reg::R11);
                // 4. arg_start
                asm.mov_reg_imm64(Reg::R11, arg_start as u64);
                asm.push(Reg::R11);
                // 5. callee
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

                emit_store(&mut asm, Reg::R11, dest, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(dest));
            }
            OpCode::CallMethod => {
                let cs = first_reg;
                let w1 = code[ip];
                ip += 1;
                let name_idx = code[ip] as usize;
                ip += 1;
                let w3 = code[ip];
                ip += 1;
                let dest = (w1 >> 8) as usize;
                let obj_reg = (w1 & 0xFF) as usize;
                let arg_count = (w3 >> 8) as usize;
                let arg_start = (w3 & 0xFF) as usize;

                emit_flush_all(&mut asm, &regmap);

                emit_load(&mut asm, Reg::Rax, obj_reg, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = (regmap.used_phys.len() + 7) % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                // Push JitCallMethodArgs struct fields in reverse order:
                // 1. ip
                asm.mov_reg_imm64(Reg::R11, ip as u64);
                asm.push(Reg::R11);
                // 2. dest
                asm.mov_reg_imm64(Reg::R11, dest as u64);
                asm.push(Reg::R11);
                // 3. arg_count
                asm.mov_reg_imm64(Reg::R11, arg_count as u64);
                asm.push(Reg::R11);
                // 4. arg_start
                asm.mov_reg_imm64(Reg::R11, arg_start as u64);
                asm.push(Reg::R11);
                // 5. cs
                asm.mov_reg_imm64(Reg::R11, cs as u64);
                asm.push(Reg::R11);
                // 6. name_idx
                asm.mov_reg_imm64(Reg::R11, name_idx as u64);
                asm.push(Reg::R11);
                // 7. this_val
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

                emit_store(&mut asm, Reg::R11, dest, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(dest));
            }
            OpCode::BuildArray => {
                let w1 = code[ip];
                ip += 1;
                let w2 = code[ip];
                ip += 1;
                let dest = (w1 >> 8) as usize;
                let start_reg = (w1 & 0xFF) as usize;
                let count = (w2 >> 8) as usize;

                emit_flush_all(&mut asm, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
                asm.mov_reg_imm64(ARG_CLOSURE, start_reg as u64);
                asm.mov_reg_imm64(ARG_BASE, count as u64);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_imm64(Reg::R10, helpers.build_array as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, dest, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(dest));
            }
            OpCode::ArrayLength => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;

                emit_flush_all(&mut asm, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                emit_load(&mut asm, Reg::Rax, src, &regmap);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);  // arg2: arr
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // arg1: ctx

                asm.mov_reg_imm64(Reg::R10, helpers.array_length as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::ArrayPush => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;

                emit_flush_all(&mut asm, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                emit_load(&mut asm, Reg::Rax, first_reg, &regmap); // arr
                emit_load(&mut asm, Reg::R11, src, &regmap); // val

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_BASE, Reg::R11);      // arg3: val
                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);  // arg2: arr
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // arg1: ctx

                asm.mov_reg_imm64(Reg::R10, helpers.array_push as u64);
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

                emit_reload_all(&mut asm, &regmap);
            }
            OpCode::ArrayPop => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;

                emit_flush_all(&mut asm, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                emit_load(&mut asm, Reg::Rax, src, &regmap);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);  // arg2: arr
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // arg1: ctx

                asm.mov_reg_imm64(Reg::R10, helpers.array_pop as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::ArrayExtend => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;

                emit_flush_all(&mut asm, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                emit_load(&mut asm, Reg::Rax, first_reg, &regmap); // arr
                emit_load(&mut asm, Reg::R11, src, &regmap); // src

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_BASE, Reg::R11);      // arg3: src
                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);  // arg2: arr
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // arg1: ctx

                asm.mov_reg_imm64(Reg::R10, helpers.array_extend as u64);
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

                emit_reload_all(&mut asm, &regmap);
            }
            OpCode::StrLength => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;

                emit_flush_all(&mut asm, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                emit_load(&mut asm, Reg::Rax, src, &regmap);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);  // arg2: v
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // arg1: ctx

                asm.mov_reg_imm64(Reg::R10, helpers.str_length as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::StrConcat => {
                let w1 = code[ip];
                ip += 1;
                let src1 = (w1 >> 8) as usize;
                let src2 = (w1 & 0xFF) as usize;

                emit_flush_all(&mut asm, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                emit_load(&mut asm, Reg::Rax, src1, &regmap);
                emit_load(&mut asm, Reg::R11, src2, &regmap);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_BASE, Reg::R11);      // arg3: b
                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);  // arg2: a
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // arg1: ctx

                asm.mov_reg_imm64(Reg::R10, helpers.str_concat as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::StrSlice => {
                let w1 = code[ip];
                ip += 1;
                let src1 = (w1 >> 8) as usize;
                let src2 = (w1 & 0xFF) as usize;

                emit_flush_all(&mut asm, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                emit_load(&mut asm, Reg::Rax, src1, &regmap);
                emit_load(&mut asm, Reg::R11, src2, &regmap);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_BASE, Reg::R11);      // arg3: idx
                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);  // arg2: s
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // arg1: ctx

                asm.mov_reg_imm64(Reg::R10, helpers.str_slice as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::BuildStr => {
                let w1 = code[ip];
                let count = (w1 >> 8) as usize;
                ip += 1;

                let original_ip = ip;
                ip += count;

                let mut reg_indices = Vec::with_capacity(count);
                for i in 0..count {
                    let w_reg = code[original_ip + i];
                    let r_idx = (w_reg >> 8) as usize;
                    reg_indices.push(r_idx);
                }

                emit_flush_all(&mut asm, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = (regmap.used_phys.len() + count) % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                for &r_idx in reg_indices.iter().rev() {
                    emit_load(&mut asm, Reg::Rax, r_idx, &regmap);
                    asm.push(Reg::Rax);
                }

                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rsp);
                asm.mov_reg_imm64(ARG_BASE, count as u64);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_imm64(Reg::R10, helpers.build_str as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                asm.add_reg_imm32(Reg::Rsp, (count * 8) as i32);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::GetIndex => {
                let w1 = code[ip];
                ip += 1;
                let obj_reg = (w1 >> 8) as usize;
                let key_reg = (w1 & 0xFF) as usize;

                emit_flush_all(&mut asm, &regmap);

                emit_load(&mut asm, Reg::Rax, obj_reg, &regmap);
                emit_load(&mut asm, Reg::R11, key_reg, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = (regmap.used_phys.len() + 3) % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                // Push JitGetIndexArgs struct in reverse: dest, key, obj
                asm.mov_reg_imm64(Reg::R10, first_reg as u64);
                asm.push(Reg::R10);
                asm.push(Reg::R11);  // key
                asm.push(Reg::Rax);  // obj

                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rsp);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_imm64(Reg::R10, helpers.get_index as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                asm.add_reg_imm8(Reg::Rsp, 24); // pop 3 * 8 bytes (obj, key, dest)

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::SetIndex => {
                let w1 = code[ip];
                ip += 1;
                let idx_reg = (w1 >> 8) as usize;
                let val_reg = (w1 & 0xFF) as usize;
                let obj_reg = first_reg;

                emit_flush_all(&mut asm, &regmap);

                emit_load(&mut asm, Reg::Rax, obj_reg, &regmap);
                emit_load(&mut asm, Reg::R11, idx_reg, &regmap);
                emit_load(&mut asm, Reg::R12, val_reg, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = (regmap.used_phys.len() + 3) % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                // Push JitSetIndexArgs: val, key, obj
                asm.push(Reg::R12);  // val
                asm.push(Reg::R11);  // key
                asm.push(Reg::Rax);  // obj

                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rsp);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_imm64(Reg::R10, helpers.set_index as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.add_reg_imm8(Reg::Rsp, 24); // pop 3 * 8 bytes

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_reload_all(&mut asm, &regmap);
            }
            OpCode::Typeof => {
                let w1 = code[ip];
                ip += 1;
                let src = (w1 >> 8) as usize;

                emit_flush_all(&mut asm, &regmap);

                emit_load(&mut asm, Reg::Rax, src, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);  // arg2: v
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // arg1: ctx

                asm.mov_reg_imm64(Reg::R10, helpers.typeof_val as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
                emit_reload_all_except(&mut asm, &regmap, Some(first_reg));
            }
            OpCode::Instanceof => {
                let w1 = code[ip];
                ip += 1;
                let src1 = (w1 >> 8) as usize;
                let src2 = (w1 & 0xFF) as usize;

                emit_load(&mut asm, Reg::Rax, src1, &regmap);
                emit_load(&mut asm, Reg::R11, src2, &regmap);

                asm.push(ARG_CTX);
                asm.push(ARG_CLOSURE);
                asm.push(ARG_BASE);
                asm.push(ARG_EXEC_CTX);

                let need_dummy = regmap.used_phys.len() % 2 != 0;
                if need_dummy {
                    asm.push(Reg::Rax);
                }

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, -32);

                asm.mov_reg_reg(ARG_BASE, Reg::R11);      // arg3: b
                asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);  // arg2: a
                asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);  // arg1: ctx

                asm.mov_reg_imm64(Reg::R10, helpers.instanceof as u64);
                asm.call_reg(Reg::R10);

                #[cfg(target_os = "windows")]
                asm.add_reg_imm8(Reg::Rsp, 32);

                asm.mov_reg_reg(Reg::R11, Reg::Rax);

                if need_dummy {
                    asm.pop(Reg::Rax);
                }
                asm.pop(ARG_EXEC_CTX);
                asm.pop(ARG_BASE);
                asm.pop(ARG_CLOSURE);
                asm.pop(ARG_CTX);

                emit_store(&mut asm, Reg::R11, first_reg, &regmap);
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
