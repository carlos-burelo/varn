use crate::codegen::ir::{ImmValue, IrBuilder, RegId};
use varn_core::opcode::OpCode;

pub struct IrExprCompiler {
    builder: IrBuilder,
}

impl IrExprCompiler {
    pub fn new() -> Self {
        Self {
            builder: IrBuilder::new(),
        }
    }

    pub fn compile_int_literal(&mut self, value: i64) -> RegId {
        let dest = self.builder.alloc_vreg();

        if value >= i16::MIN as i64 && value <= i16::MAX as i64 {
            self.builder
                .emit_unary(OpCode::LoadInt, dest, ImmValue::Small(value as i16));
        } else {
            self.builder
                .emit_unary(OpCode::LoadInt, dest, ImmValue::Large(value as i32));
        }

        dest
    }

    pub fn compile_binary_add(&mut self, left_val: RegId, right_val: RegId) -> RegId {
        let dest = self.builder.alloc_vreg();

        self.builder
            .emit_binary(OpCode::Add, dest, left_val, right_val);

        dest
    }

    pub fn compile_load_var(&mut self, _var_name: &str) -> RegId {
        let dest = self.builder.alloc_vreg();

        self.builder
            .emit_unary(OpCode::LoadGlobal, dest, ImmValue::Small(0));

        dest
    }

    pub fn finish(self) -> IrCompilationResult {
        let vreg_count = self.builder.vreg_count();
        let module = self.builder.finish();
        let bytecode_size_estimate = module.estimate_bytecode_size();

        IrCompilationResult {
            ir: module,
            estimated_bytecode_size: bytecode_size_estimate,
            vreg_count,
        }
    }
}

pub struct IrCompilationResult {
    pub ir: crate::codegen::ir::IrModule,
    pub estimated_bytecode_size: usize,
    pub vreg_count: u16,
}

impl IrCompilationResult {
    pub fn vregs_used(&self) -> Vec<u16> {
        self.ir.used_vregs()
    }

    pub fn instruction_count(&self) -> usize {
        self.ir.instrs.len()
    }

    pub fn estimate_physical_regs_needed(&self, max_live_at_once: u8) -> u8 {
        std::cmp::min(max_live_at_once, 255)
    }
}
