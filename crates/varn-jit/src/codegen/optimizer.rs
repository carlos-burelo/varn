use crate::assembler::Assembler;

/// Peephole optimizer that performs in-place byte-level optimizations on
/// generated x86-64 machine code.  Runs **after** jump patches, so it must
/// never change code size – only replace instructions with NOPs or
/// equivalent-size alternatives.
pub fn optimize(asm: &mut Assembler) -> Result<(), String> {
    let code = asm.code_mut();
    let len = code.len();
    let mut i = 0;

    while i + 2 < len {
        let b0 = code[i];

        // ── Pattern 1: `mov rX, rX` (same 64-bit register) → 3× NOP ──
        //
        // Encoding: REX.W (0x48-0x4F) + 0x89 + ModRM
        //   REX byte: 0100 W R X B  (W=1 → bits 0x48..0x4F)
        //   ModRM:    mode=11, reg, rm
        //   full_reg = (modrm>>3 & 7) | ((rex & 0x04) << 1)
        //   full_rm  = (modrm     & 7) | ((rex & 0x01) << 3)
        if (b0 & 0xF8) == 0x48 {
            // REX.W prefix (W bit set, other bits vary)
            let rex = b0;
            if i + 2 < len && code[i + 1] == 0x89 {
                let modrm = code[i + 2];
                let mode = modrm >> 6;
                if mode == 0b11 {
                    let full_reg = ((modrm >> 3) & 7) | ((rex & 0x04) << 1);
                    let full_rm = (modrm & 7) | ((rex & 0x01) << 3);
                    if full_reg == full_rm {
                        code[i] = 0x90;
                        code[i + 1] = 0x90;
                        code[i + 2] = 0x90;
                        i += 3;
                        continue;
                    }
                }
            }

            // ── Pattern 2: `movabs rX, 0` → `xor eX, eX` + NOP padding ──
            //
            // movabs encoding: REX.W + (0xB8 + reg_lo) + 8-byte immediate
            //   total = 10 bytes
            //   reg_lo = opcode & 7
            //   full_reg = reg_lo | ((rex & 0x01) << 3)
            //
            // xor r32, r32 zero-extends to 64-bit.
            // For regs 0-7: 2 bytes (0x31 + ModRM)
            // For regs 8-15: 3 bytes (REX + 0x31 + ModRM)
            if i + 9 < len {
                let opbyte = code[i + 1];
                if (opbyte & 0xF8) == 0xB8 {
                    // Check if the 8-byte immediate is zero
                    let mut is_zero = true;
                    for j in 0..8 {
                        if code[i + 2 + j] != 0 {
                            is_zero = false;
                            break;
                        }
                    }
                    if is_zero {
                        let reg_lo = opbyte & 7;
                        let full_reg = reg_lo | ((rex & 0x01) << 3);

                        if full_reg >= 8 {
                            // Need REX prefix for extended registers
                            // REX.R and REX.B for same register
                            let rex_xor = 0x41 | ((full_reg & 8) >> 3) | (((full_reg & 8) >> 3) << 2);
                            let modrm = 0xC0 | ((full_reg & 7) << 3) | (full_reg & 7);
                            code[i] = rex_xor;
                            code[i + 1] = 0x31;
                            code[i + 2] = modrm;
                            // NOP-fill the remaining 7 bytes
                            for j in 3..10 {
                                code[i + j] = 0x90;
                            }
                        } else {
                            // Standard registers (0-7), no REX needed
                            let modrm = 0xC0 | (reg_lo << 3) | reg_lo;
                            code[i] = 0x31;
                            code[i + 1] = modrm;
                            // NOP-fill the remaining 8 bytes
                            for j in 2..10 {
                                code[i + j] = 0x90;
                            }
                        }
                        i += 10;
                        continue;
                    }
                }
            }
        }

        i += 1;
    }

    Ok(())
}
