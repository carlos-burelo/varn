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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Cond {
    Overflow = 0,
    NoOverflow = 1,
    Below = 2,
    AboveEqual = 3,
    Equal = 4,
    NotEqual = 5,
    BelowEqual = 6,
    Above = 7,
    Sign = 8,
    NoSign = 9,
    Parity = 10,
    NoParity = 11,
    Less = 12,
    GreaterEqual = 13,
    LessEqual = 14,
    Greater = 15,
}

pub struct Assembler {
    code: Vec<u8>,
}

impl Assembler {
    pub fn new() -> Self {
        Self {
            code: Vec::with_capacity(256),
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.code
    }

    pub fn bytes(&self) -> &[u8] {
        &self.code
    }

    pub fn current_offset(&self) -> usize {
        self.code.len()
    }

    pub fn patch_u32(&mut self, offset: usize, value: u32) {
        let bytes = value.to_le_bytes();
        self.code[offset] = bytes[0];
        self.code[offset + 1] = bytes[1];
        self.code[offset + 2] = bytes[2];
        self.code[offset + 3] = bytes[3];
    }

    fn emit_byte(&mut self, b: u8) {
        self.code.push(b);
    }

    fn emit_u32(&mut self, v: u32) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }

    fn emit_u64(&mut self, v: u64) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }

    fn emit_rex(&mut self, w: bool, r: u8, x: u8, b: u8) {
        let r_bit = (r & 8) >> 1;
        let x_bit = (x & 8) >> 2;
        let b_bit = (b & 8) >> 3;
        let w_bit = if w { 8 } else { 0 };

        let rex = 0x40 | w_bit | r_bit | x_bit | b_bit;
        if rex != 0x40 || w {
            self.emit_byte(rex);
        }
    }

    fn emit_modrm(&mut self, mode: u8, reg: u8, rm: u8) {
        let modrm = ((mode & 3) << 6) | ((reg & 7) << 3) | (rm & 7);
        self.emit_byte(modrm);
    }

    fn emit_mem_address(&mut self, reg_op: u8, base: Reg, offset: i32) {
        let base_val = base as u8;
        let requires_sib = (base_val & 7) == Reg::Rsp as u8 || (base_val & 7) == Reg::R12 as u8;

        let is_ebp_r13 = (base_val & 7) == Reg::Rbp as u8 || (base_val & 7) == Reg::R13 as u8;

        if offset == 0 && !is_ebp_r13 {
            self.emit_modrm(0b00, reg_op, base_val);
            if requires_sib {
                self.emit_byte(0x24);
            }
        } else if offset >= -128 && offset <= 127 {
            self.emit_modrm(0b01, reg_op, base_val);
            if requires_sib {
                self.emit_byte(0x24);
            }
            self.emit_byte(offset as u8);
        } else {
            self.emit_modrm(0b10, reg_op, base_val);
            if requires_sib {
                self.emit_byte(0x24);
            }
            self.emit_u32(offset as u32);
        }
    }

    pub fn mov_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, src as u8, 0, dest as u8);
        self.emit_byte(0x89);
        self.emit_modrm(0b11, src as u8, dest as u8);
    }

    pub fn mov_reg_imm64(&mut self, dest: Reg, imm: u64) {
        self.emit_rex(true, 0, 0, dest as u8);
        self.emit_byte(0xB8 + (dest as u8 & 7));
        self.emit_u64(imm);
    }

    pub fn mov_reg_mem(&mut self, dest: Reg, base: Reg, offset: i32) {
        self.emit_rex(true, dest as u8, 0, base as u8);
        self.emit_byte(0x8B);
        self.emit_mem_address(dest as u8, base, offset);
    }

    pub fn mov_mem_reg(&mut self, base: Reg, offset: i32, src: Reg) {
        self.emit_rex(true, src as u8, 0, base as u8);
        self.emit_byte(0x89);
        self.emit_mem_address(src as u8, base, offset);
    }

    pub fn push(&mut self, reg: Reg) {
        let reg_val = reg as u8;
        if reg_val >= 8 {
            self.emit_rex(false, 0, 0, reg_val);
        }
        self.emit_byte(0x50 + (reg_val & 7));
    }

    pub fn pop(&mut self, reg: Reg) {
        let reg_val = reg as u8;
        if reg_val >= 8 {
            self.emit_rex(false, 0, 0, reg_val);
        }
        self.emit_byte(0x58 + (reg_val & 7));
    }

    pub fn add_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, src as u8, 0, dest as u8);
        self.emit_byte(0x01);
        self.emit_modrm(0b11, src as u8, dest as u8);
    }

    pub fn add_reg_imm32(&mut self, reg: Reg, imm: i32) {
        self.emit_rex(true, 0, 0, reg as u8);
        self.emit_byte(0x81);
        self.emit_modrm(0b11, 0, reg as u8);
        self.emit_u32(imm as u32);
    }

    pub fn add_reg_imm8(&mut self, reg: Reg, imm: i8) {
        self.emit_rex(true, 0, 0, reg as u8);
        self.emit_byte(0x83);
        self.emit_modrm(0b11, 0, reg as u8);
        self.emit_byte(imm as u8);
    }

    pub fn shl_reg_imm8(&mut self, reg: Reg, imm: u8) {
        self.emit_rex(true, 0, 0, reg as u8);
        self.emit_byte(0xC1);
        self.emit_modrm(0b11, 4, reg as u8);
        self.emit_byte(imm);
    }

    pub fn sar_reg_imm8(&mut self, reg: Reg, imm: u8) {
        self.emit_rex(true, 0, 0, reg as u8);
        self.emit_byte(0xC1);
        self.emit_modrm(0b11, 7, reg as u8);
        self.emit_byte(imm);
    }

    pub fn shr_reg_imm8(&mut self, reg: Reg, imm: u8) {
        self.emit_rex(true, 0, 0, reg as u8);
        self.emit_byte(0xC1);
        self.emit_modrm(0b11, 5, reg as u8);
        self.emit_byte(imm);
    }

    pub fn and_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, src as u8, 0, dest as u8);
        self.emit_byte(0x21);
        self.emit_modrm(0b11, src as u8, dest as u8);
    }

    pub fn or_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, src as u8, 0, dest as u8);
        self.emit_byte(0x09);
        self.emit_modrm(0b11, src as u8, dest as u8);
    }

    pub fn xor_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, src as u8, 0, dest as u8);
        self.emit_byte(0x31);
        self.emit_modrm(0b11, src as u8, dest as u8);
    }

    pub fn sub_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, src as u8, 0, dest as u8);
        self.emit_byte(0x29);
        self.emit_modrm(0b11, src as u8, dest as u8);
    }

    pub fn imul_reg_reg(&mut self, dest: Reg, src: Reg) {
        self.emit_rex(true, dest as u8, 0, src as u8);
        self.emit_byte(0x0F);
        self.emit_byte(0xAF);
        self.emit_modrm(0b11, dest as u8, src as u8);
    }

    pub fn cmp_reg_reg(&mut self, lhs: Reg, rhs: Reg) {
        self.emit_rex(true, rhs as u8, 0, lhs as u8);
        self.emit_byte(0x39);
        self.emit_modrm(0b11, rhs as u8, lhs as u8);
    }

    pub fn cmp_reg_imm32(&mut self, lhs: Reg, imm: i32) {
        self.emit_rex(true, 0, 0, lhs as u8);
        self.emit_byte(0x81);
        self.emit_modrm(0b11, 7, lhs as u8);
        self.emit_u32(imm as u32);
    }

    pub fn jmp_cond(&mut self, cond: Cond) -> usize {
        self.emit_byte(0x0F);
        self.emit_byte(0x80 + cond as u8);
        let patch_pos = self.current_offset();
        self.emit_u32(0);
        patch_pos
    }

    pub fn jmp_near(&mut self) -> usize {
        self.emit_byte(0xE9);
        let patch_pos = self.current_offset();
        self.emit_u32(0);
        patch_pos
    }

    pub fn call_reg(&mut self, reg: Reg) {
        let reg_val = reg as u8;
        self.emit_rex(false, 0, 0, reg_val);
        self.emit_byte(0xFF);
        self.emit_modrm(0b11, 2, reg_val);
    }

    pub fn ret(&mut self) {
        self.emit_byte(0xC3);
    }
}
