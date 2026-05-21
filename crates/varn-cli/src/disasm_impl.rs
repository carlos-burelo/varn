use varn_core::OpCode;
use varn_types::{Chunk, FunctionProto, PoolEntry};

pub struct DisassemblerImpl<'a> {
    pub chunk: &'a Chunk,
}

pub fn print(proto: &FunctionProto) {
    let disasm = DisassemblerImpl {
        chunk: &proto.chunk,
    };
    disasm.disassemble_recursive(proto);
}

pub trait Disassembler {
    fn disassemble(&self);
    fn disassemble_instruction(&self, offset: usize) -> usize;
}

impl<'a> Disassembler for DisassemblerImpl<'a> {
    fn disassemble(&self) {
        let mut offset = 0;
        while offset < self.chunk.code.len() {
            offset = self.disassemble_instruction(offset);
        }
    }

    fn disassemble_instruction(&self, offset: usize) -> usize {
        let code = &self.chunk.code;
        print!("{:04} ", offset);
        let line = self.chunk.lines.get_line(offset);
        if offset > 0 && line == self.chunk.lines.get_line(offset - 1) {
            print!("   | ");
        } else {
            print!("{:4} ", line);
        }

        let instruction = code[offset];
        let first_reg = (instruction >> 8) as usize;
        let op = match OpCode::from_u16(instruction) {
            Some(op) => op,
            None => {
                println!("<unknown {}>", instruction);
                return offset + 1;
            }
        };

        match op {
            OpCode::PopTry
            | OpCode::Nop
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne
            | OpCode::LoadNull
            | OpCode::LoadTrue
            | OpCode::LoadFalse => {
                println!("{:?} r{}", op, first_reg);
                offset + 1
            }

            OpCode::Move
            | OpCode::Negate
            | OpCode::Not
            | OpCode::ToString
            | OpCode::IsNull
            | OpCode::IsArray
            | OpCode::Typeof
            | OpCode::AssertNotNull
            | OpCode::WrapSpread
            | OpCode::ArrayLength
            | OpCode::ArrayPush
            | OpCode::ArrayPop
            | OpCode::StrLength
            | OpCode::GetEnumTag
            | OpCode::Await
            | OpCode::Yield
            | OpCode::Return
            | OpCode::Throw
            | OpCode::MergeExports
            | OpCode::LoadUpvalue
            | OpCode::StoreUpvalue
            | OpCode::CloseUpvalue
            | OpCode::Inherit
            | OpCode::Jump
            | OpCode::Loop => {
                let hi = code.get(offset + 1).copied().unwrap_or(0) as u32;
                let lo = code.get(offset + 2).copied().unwrap_or(0) as u32;
                let jump_offset = (hi << 16) | lo;
                println!("{:?} {}", op, jump_offset);
                offset + 3
            }

            OpCode::LoadConst
            | OpCode::LoadInt
            | OpCode::LoadGlobal
            | OpCode::StoreGlobal
            | OpCode::DefineGlobal
            | OpCode::DefineGlobalIdx
            | OpCode::LoadGlobalIdx
            | OpCode::StoreGlobalIdx
            | OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::Ushr
            | OpCode::StrConcat
            | OpCode::StrSlice
            | OpCode::In
            | OpCode::Instanceof
            | OpCode::ArrayExtend
            | OpCode::ObjectKeys
            | OpCode::ObjectMerge
            | OpCode::GetIndex
            | OpCode::SetIndex
            | OpCode::BuildArray
            | OpCode::BuildObjectWithShape
            | OpCode::GetProperty
            | OpCode::GetPropertyMaybe
            | OpCode::SetProperty
            | OpCode::GetFixedField
            | OpCode::SetFixedField
            | OpCode::GetSuper
            | OpCode::GetSymbol
            | OpCode::BindMethod
            | OpCode::MakeClass
            | OpCode::DeclareField
            | OpCode::MakeEnumVariant
            | OpCode::Import
            | OpCode::Reexport
            | OpCode::Call
            | OpCode::CallSpread
            | OpCode::InvokeVirtual
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue => {
                let hi = code.get(offset + 1).copied().unwrap_or(0) as u32;
                let lo = code.get(offset + 2).copied().unwrap_or(0) as u32;
                let jump_offset = (hi << 16) | lo;
                println!("{:?} r{} +{}", op, first_reg, jump_offset);
                offset + 3
            }

            OpCode::Try => {
                let w1 = code.get(offset + 1).copied().unwrap_or(0);
                let err_reg = (w1 >> 8) as usize;
                let hi = code.get(offset + 2).copied().unwrap_or(0) as u32;
                let lo = code.get(offset + 3).copied().unwrap_or(0) as u32;
                let catch_offset = (hi << 16) | lo;
                println!("{:?} err=r{} catch=+{}", op, err_reg, catch_offset);
                offset + 4
            }

            OpCode::Method
            | OpCode::DefineStatic
            | OpCode::DefineGetter
            | OpCode::DefineSetter
            | OpCode::DefineStaticGetter
            | OpCode::DefineStaticSetter => {
                let w1 = code.get(offset + 1).copied().unwrap_or(0);
                let w2 = code.get(offset + 2).copied().unwrap_or(0);
                println!("{:?} {} {}", op, w1, w2);
                offset + 3
            }

            OpCode::CallMethod => {
                let w1 = code.get(offset + 1).copied().unwrap_or(0);
                let w2 = code.get(offset + 2).copied().unwrap_or(0);
                let w3 = code.get(offset + 3).copied().unwrap_or(0);
                println!("{:?} {} {} {}", op, w1, w2, w3);
                offset + 4
            }

            OpCode::MakeClosure => {
                let w1 = code.get(offset + 1).copied().unwrap_or(0);
                let proto_idx = code.get(offset + 2).copied().unwrap_or(0);
                let dest = (w1 >> 8) as usize;
                let uv_count = (w1 & 0xff) as usize;
                println!(
                    "{:?} {} {} (dest={}, uv_count={})",
                    op, w1, proto_idx, dest, uv_count
                );
                offset + 3 + uv_count
            }

            OpCode::BuildObject => {
                let w1 = code.get(offset + 1).copied().unwrap_or(0);
                let dest = (w1 >> 8) as usize;
                let count = (w1 & 0xff) as usize;
                println!("{:?} dest={} count={}", op, dest, count);
                offset + 2 + count * 2
            }

            OpCode::ObjectRest => {
                let w1 = code.get(offset + 1).copied().unwrap_or(0);
                let w2 = code.get(offset + 2).copied().unwrap_or(0);
                let skip_count = (w2 & 0xff) as usize;
                println!("{:?} {} {} (skip={})", op, w1, w2, skip_count);
                offset + 3 + skip_count
            }

            OpCode::InvokeRuntimeStatic => {
                let w1 = code.get(offset + 1).copied().unwrap_or(0);
                let w2 = code.get(offset + 2).copied().unwrap_or(0);
                let arg_count = (w2 & 0xff) as usize;
                println!("{:?} {} {} (args={})", op, w1, w2, arg_count);
                offset + 3 + arg_count
            }

            OpCode::Spawn => {
                let w1 = code.get(offset + 1).copied().unwrap_or(0);
                let w2 = code.get(offset + 2).copied().unwrap_or(0);
                println!("{:?} {} {}", op, w1, w2);
                offset + 3
            }

            OpCode::AddImm | OpCode::SubImm => {
                let dest = instruction >> 8;
                let w1 = code.get(offset + 1).copied().unwrap_or(0);
                let src = w1 >> 8;
                let imm = (w1 & 0xff) as u8 as i8;
                let sign = if matches!(op, OpCode::SubImm) { "-" } else { "+" };
                println!("r{} = r{} {} {}", dest, src, sign, imm);
                offset + 2
            }

            OpCode::BuildStr => {
                let dest = instruction >> 8;
                let w1 = code.get(offset + 1).copied().unwrap_or(0);
                let count = (w1 >> 8) as usize;
                let mut reg_strs = Vec::with_capacity(count);
                for i in 0..count {
                    let w = code.get(offset + 2 + i).copied().unwrap_or(0);
                    reg_strs.push(format!("r{}", w >> 8));
                }
                println!("r{} = concat({})", dest, reg_strs.join(", "));
                offset + 2 + count
            }
        }
    }
}

impl<'a> DisassemblerImpl<'a> {
    pub fn disassemble_recursive(&self, proto: &FunctionProto) {
        println!(
            "== {} == (register_count={})",
            proto.name.as_deref().unwrap_or("<top-level>"),
            proto.register_count
        );
        let disasm = DisassemblerImpl {
            chunk: &proto.chunk,
        };
        disasm.disassemble();
        println!();

        for entry in &proto.chunk.constants {
            if let PoolEntry::Function(f) = entry {
                self.disassemble_recursive(f);
            }
        }
    }
}
