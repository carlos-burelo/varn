use crate::assembler::Assembler;

pub fn optimize(asm: &mut Assembler) -> Result<(), String> {
    let code = asm.code_mut();
    let len = code.len();
    let mut i = 0;

    while i + 2 < len {
        let b0 = code[i];

        if (b0 & 0xF8) == 0x48 {
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

            if i + 9 < len {
                let opbyte = code[i + 1];
                if (opbyte & 0xF8) == 0xB8 {
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
                            let rex_xor =
                                0x41 | ((full_reg & 8) >> 3) | (((full_reg & 8) >> 3) << 2);
                            let modrm = 0xC0 | ((full_reg & 7) << 3) | (full_reg & 7);
                            code[i] = rex_xor;
                            code[i + 1] = 0x31;
                            code[i + 2] = modrm;

                            for j in 3..10 {
                                code[i + j] = 0x90;
                            }
                        } else {
                            let modrm = 0xC0 | (reg_lo << 3) | reg_lo;
                            code[i] = 0x31;
                            code[i + 1] = modrm;

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
