/// Representation of x86_64 64-bit general-purpose registers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Reg {
    Rax = 0,
    Rcx = 1,
    Rdx = 2,
    Rbx = 3,
    Rsp = 4,
    Rbp = 5,
    Rsi = 6,
    Rdi = 7,
    R8 = 8,
    R9 = 9,
    R10 = 10,
    R11 = 11,
    R12 = 12,
    R13 = 13,
    R14 = 14,
    R15 = 15,
}

/// Representation of x86_64 conditional jump conditions.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Cond {
    Overflow = 0,
    NoOverflow = 1,
    Below = 2,      // Unsigned <
    AboveEqual = 3, // Unsigned >=
    Equal = 4,      // ==
    NotEqual = 5,   // !=
    BelowEqual = 6, // Unsigned <=
    Above = 7,      // Unsigned >
    Sign = 8,
    NoSign = 9,
    Parity = 10,
    NoParity = 11,
    Less = 12,         // Signed <
    GreaterEqual = 13, // Signed >=
    LessEqual = 14,    // Signed <=
    Greater = 15,      // Signed >
}

/// A lightweight, clean machine code emitter for x86_64.
pub struct Assembler {
    code: Vec<u8>,
}

impl Assembler {
    /// Creates a new, empty assembler.
    pub fn new() -> Self {
        Self {
            code: Vec::with_capacity(256),
        }
    }

    /// Consumes the assembler and returns the generated machine code bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.code
    }

    /// Returns a reference to the current emitted code.
    pub fn bytes(&self) -> &[u8] {
        &self.code
    }

    /// Returns the current offset (IP) in bytes.
    pub fn current_offset(&self) -> usize {
        self.code.len()
    }

    /// Overwrites 4 bytes at a given offset. Useful for patching jump targets.
    pub fn patch_u32(&mut self, offset: usize, value: u32) {
        let bytes = value.to_le_bytes();
        self.code[offset] = bytes[0];
        self.code[offset + 1] = bytes[1];
        self.code[offset + 2] = bytes[2];
        self.code[offset + 3] = bytes[3];
    }

    // --- x86_64 Encoding Helpers ---

    fn emit_byte(&mut self, b: u8) {
        self.code.push(b);
    }

    fn emit_u32(&mut self, v: u32) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }

    fn emit_u64(&mut self, v: u64) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }

    /// Emits a REX prefix if any extension or 64-bit operand is specified.
    /// `w` = 64-bit operand size
    /// `r` = Reg field extension (ModRM reg bit 3)
    /// `x` = SIB index field extension (SIB index bit 3)
    /// `b` = ModRM r/m or SIB base field extension (ModRM r/m bit 3 or SIB base bit 3)
    fn emit_rex(&mut self, w: bool, r: u8, x: u8, b: u8) {
        let r_bit = (r & 8) >> 1; // Bit 2 of REX
        let x_bit = (x & 8) >> 2; // Bit 1 of REX
        let b_bit = (b & 8) >> 3; // Bit 0 of REX
        let w_bit = if w { 8 } else { 0 };

        let rex = 0x40 | w_bit | r_bit | x_bit | b_bit;
        if rex != 0x40 || w {
            self.emit_byte(rex);
        }
    }

    /// Emits a ModR/M byte.
    /// `mode` = 2 bits
    /// `reg` = 3 bits
    /// `rm` = 3 bits
    fn emit_modrm(&mut self, mode: u8, reg: u8, rm: u8) {
        let modrm = ((mode & 3) << 6) | ((reg & 7) << 3) | (rm & 7);
        self.emit_byte(modrm);
    }

    /// Emits ModR/M and optional SIB and displacement bytes for memory addressing.
    fn emit_mem_address(&mut self, reg_op: u8, base: Reg, offset: i32) {
        let base_val = base as u8;
        let requires_sib = (base_val & 7) == Reg::Rsp as u8 || (base_val & 7) == Reg::R12 as u8;

        // If offset is 0 and base is Rbp/R13, we MUST use 8-bit displacement [Rbp + 0]
        // because ModR/M with mod=00 and r/m=Rbp means RIP-relative.
        let is_ebp_r13 = (base_val & 7) == Reg::Rbp as u8 || (base_val & 7) == Reg::R13 as u8;

        if offset == 0 && !is_ebp_r13 {
            // [base] - ModRM with mod = 00
            self.emit_modrm(0b00, reg_op, base_val);
            if requires_sib {
                // SIB byte: scale=0, index=none (0x04 / RSP), base=RSP/R12
                self.emit_byte(0x24);
            }
        } else if offset >= -128 && offset <= 127 {
            // [base + disp8] - ModRM with mod = 0b01
            self.emit_modrm(0b01, reg_op, base_val);
            if requires_sib {
                self.emit_byte(0x24);
            }
            self.emit_byte(offset as u8);
        } else {
            // [base + disp32] - ModRM with mod = 0b10
            self.emit_modrm(0b10, reg_op, base_val);
            if requires_sib {
                self.emit_byte(0x24);
            }
            self.emit_u32(offset as u32);
        }
    }

    // --- Public Instruction Set API ---

    /// `mov dest, src` (64-bit register to register)
    pub fn mov_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, src as u8, 0, dest as u8);
        self.emit_byte(0x89);
        self.emit_modrm(0b11, src as u8, dest as u8);
    }

    /// `mov dest, imm64` (64-bit immediate to register)
    pub fn mov_reg_imm64(&mut self, dest: Reg, imm: u64) {
        self.emit_rex(true, 0, 0, dest as u8);
        self.emit_byte(0xB8 + (dest as u8 & 7));
        self.emit_u64(imm);
    }

    /// `mov dest, [base + offset]` (64-bit load from memory)
    pub fn mov_reg_mem(&mut self, dest: Reg, base: Reg, offset: i32) {
        self.emit_rex(true, dest as u8, 0, base as u8);
        self.emit_byte(0x8B);
        self.emit_mem_address(dest as u8, base, offset);
    }

    /// `mov [base + offset], src` (64-bit store to memory)
    pub fn mov_mem_reg(&mut self, base: Reg, offset: i32, src: Reg) {
        self.emit_rex(true, src as u8, 0, base as u8);
        self.emit_byte(0x89);
        self.emit_mem_address(src as u8, base, offset);
    }

    /// `push reg` (64-bit push to hardware stack)
    pub fn push(&mut self, reg: Reg) {
        let reg_val = reg as u8;
        if reg_val >= 8 {
            self.emit_rex(false, 0, 0, reg_val);
        }
        self.emit_byte(0x50 + (reg_val & 7));
    }

    /// `pop reg` (64-bit pop from hardware stack)
    pub fn pop(&mut self, reg: Reg) {
        let reg_val = reg as u8;
        if reg_val >= 8 {
            self.emit_rex(false, 0, 0, reg_val);
        }
        self.emit_byte(0x58 + (reg_val & 7));
    }

    /// `add dest, src` (64-bit register arithmetic)
    pub fn add_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, src as u8, 0, dest as u8);
        self.emit_byte(0x01);
        self.emit_modrm(0b11, src as u8, dest as u8);
    }

    /// `add reg, imm32`
    pub fn add_reg_imm32(&mut self, reg: Reg, imm: i32) {
        self.emit_rex(true, 0, 0, reg as u8);
        self.emit_byte(0x81);
        self.emit_modrm(0b11, 0, reg as u8); // /0 is add extension
        self.emit_u32(imm as u32);
    }

    /// `add reg, imm8`
    pub fn add_reg_imm8(&mut self, reg: Reg, imm: i8) {
        self.emit_rex(true, 0, 0, reg as u8);
        self.emit_byte(0x83);
        self.emit_modrm(0b11, 0, reg as u8); // /0 is add extension
        self.emit_byte(imm as u8);
    }

    /// `shl reg, imm8` (64-bit shift left)
    pub fn shl_reg_imm8(&mut self, reg: Reg, imm: u8) {
        self.emit_rex(true, 0, 0, reg as u8);
        self.emit_byte(0xC1);
        self.emit_modrm(0b11, 4, reg as u8); // /4 is shl extension
        self.emit_byte(imm);
    }

    /// `sar reg, imm8` (64-bit arithmetic shift right)
    pub fn sar_reg_imm8(&mut self, reg: Reg, imm: u8) {
        self.emit_rex(true, 0, 0, reg as u8);
        self.emit_byte(0xC1);
        self.emit_modrm(0b11, 7, reg as u8); // /7 is sar extension
        self.emit_byte(imm);
    }

    /// `shr reg, imm8` (64-bit logical shift right)
    pub fn shr_reg_imm8(&mut self, reg: Reg, imm: u8) {
        self.emit_rex(true, 0, 0, reg as u8);
        self.emit_byte(0xC1);
        self.emit_modrm(0b11, 5, reg as u8); // /5 is shr extension
        self.emit_byte(imm);
    }

    /// `and dest, src` (64-bit register bitwise AND)
    pub fn and_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, src as u8, 0, dest as u8);
        self.emit_byte(0x21);
        self.emit_modrm(0b11, src as u8, dest as u8);
    }

    /// `or dest, src` (64-bit register bitwise OR)
    pub fn or_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, src as u8, 0, dest as u8);
        self.emit_byte(0x09);
        self.emit_modrm(0b11, src as u8, dest as u8);
    }

    /// `xor dest, src` (64-bit register bitwise XOR)
    pub fn xor_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, src as u8, 0, dest as u8);
        self.emit_byte(0x31);
        self.emit_modrm(0b11, src as u8, dest as u8);
    }

    /// `sub dest, src` (64-bit register arithmetic)
    pub fn sub_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, src as u8, 0, dest as u8);
        self.emit_byte(0x29);
        self.emit_modrm(0b11, src as u8, dest as u8);
    }

    /// `imul dest, src` (64-bit signed multiply)
    pub fn imul_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, dest as u8, 0, src as u8);
        self.emit_byte(0x0F);
        self.emit_byte(0xAF);
        self.emit_modrm(0b11, dest as u8, src as u8);
    }

    /// `cmp lhs, rhs` (64-bit compare)
    pub fn cmp_reg_reg(&mut self, lhs: Reg, rhs: Reg) {
        self.emit_rex(true, rhs as u8, 0, lhs as u8);
        self.emit_byte(0x39);
        self.emit_modrm(0b11, rhs as u8, lhs as u8);
    }

    /// `cmp lhs, imm32` (64-bit compare with immediate)
    pub fn cmp_reg_imm32(&mut self, lhs: Reg, imm: i32) {
        self.emit_rex(true, 0, 0, lhs as u8);
        self.emit_byte(0x81);
        self.emit_modrm(0b11, 7, lhs as u8); // /7 is cmp extension
        self.emit_u32(imm as u32);
    }

    /// `jmp cond label_offset` (conditional jump with relative 32-bit displacement)
    /// Returns the offset in code where the 32-bit displacement is stored, for patching later.
    pub fn jmp_cond(&mut self, cond: Cond) -> usize {
        self.emit_byte(0x0F);
        self.emit_byte(0x80 + cond as u8);
        let patch_pos = self.current_offset();
        self.emit_u32(0); // Placeholder to patch
        patch_pos
    }

    /// `jmp label_offset` (unconditional relative 32-bit jump)
    /// Returns the offset in code where the 32-bit displacement is stored.
    pub fn jmp_near(&mut self) -> usize {
        self.emit_byte(0xE9);
        let patch_pos = self.current_offset();
        self.emit_u32(0); // Placeholder to patch
        patch_pos
    }

    /// `call reg` (indirect call to address in 64-bit register)
    pub fn call_reg(&mut self, reg: Reg) {
        let reg_val = reg as u8;
        self.emit_rex(false, 0, 0, reg_val);
        self.emit_byte(0xFF);
        self.emit_modrm(0b11, 2, reg_val); // /2 is call extension
    }

    /// `ret` (return from subroutine)
    pub fn ret(&mut self) {
        self.emit_byte(0xC3);
    }
}
