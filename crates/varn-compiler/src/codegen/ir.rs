use varn_core::opcode::OpCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrInstr {
    pub opcode: OpCode,
    pub dest: RegId,
    pub src1: RegId,
    pub src2: RegId,
    pub imm: ImmValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegId(pub u16);

impl RegId {
    pub const NONE: RegId = RegId(u16::MAX);

    pub fn new(id: u16) -> Self {
        RegId(id)
    }

    pub fn is_none(&self) -> bool {
        self.0 == u16::MAX
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImmValue {
    None,
    Small(i16),
    Large(i32),
    Index(u32),
}

impl ImmValue {
    pub fn is_small(&self) -> bool {
        matches!(self, ImmValue::Small(_))
    }

    pub fn small_cost(&self) -> usize {
        match self {
            ImmValue::None => 0,
            ImmValue::Small(_) => 1,
            ImmValue::Large(_) => 2,
            ImmValue::Index(_) => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IrModule {
    pub instrs: Vec<IrInstr>,
}

impl IrModule {
    pub fn new() -> Self {
        Self {
            instrs: Vec::with_capacity(256),
        }
    }

    pub fn estimate_bytecode_size(&self) -> usize {
        self.instrs
            .iter()
            .map(|instr| {
                let mut size = 2;

                size += instr.imm.small_cost();

                size
            })
            .sum()
    }

    pub fn used_vregs(&self) -> Vec<u16> {
        let mut vregs = std::collections::HashSet::new();

        for instr in &self.instrs {
            if !instr.dest.is_none() {
                vregs.insert(instr.dest.0);
            }
            if !instr.src1.is_none() {
                vregs.insert(instr.src1.0);
            }
            if !instr.src2.is_none() {
                vregs.insert(instr.src2.0);
            }
        }

        let mut vregs: Vec<_> = vregs.into_iter().collect();
        vregs.sort();
        vregs
    }
}

pub struct IrBuilder {
    module: IrModule,
    next_vreg: u16,
}

impl IrBuilder {
    pub fn new() -> Self {
        Self {
            module: IrModule::new(),
            next_vreg: 0,
        }
    }

    pub fn alloc_vreg(&mut self) -> RegId {
        let id = self.next_vreg;
        self.next_vreg += 1;
        RegId(id)
    }

    pub fn emit(&mut self, opcode: OpCode, dest: RegId, src1: RegId, src2: RegId, imm: ImmValue) {
        self.module.instrs.push(IrInstr {
            opcode,
            dest,
            src1,
            src2,
            imm,
        });
    }

    pub fn emit_unary(&mut self, opcode: OpCode, dest: RegId, imm: ImmValue) {
        self.emit(opcode, dest, RegId::NONE, RegId::NONE, imm);
    }

    pub fn emit_unary_src(&mut self, opcode: OpCode, dest: RegId, src1: RegId) {
        self.emit(opcode, dest, src1, RegId::NONE, ImmValue::None);
    }

    pub fn emit_binary(&mut self, opcode: OpCode, dest: RegId, src1: RegId, src2: RegId) {
        self.emit(opcode, dest, src1, src2, ImmValue::None);
    }

    pub fn finish(self) -> IrModule {
        self.module
    }

    pub fn vreg_count(&self) -> u16 {
        self.next_vreg
    }
}
