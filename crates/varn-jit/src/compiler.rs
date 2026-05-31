use varn_core::OpCode;
use varn_types::FunctionProto;

use crate::assembler::Assembler;
use crate::mem::JitBuffer;
use crate::regalloc::{emit_prologue, RegMap};
use crate::safepoint::emit_load_reg;

use crate::codegen::{
    emit_arith, emit_arrays, emit_calls, emit_closures, emit_compare, emit_globals,
    emit_immediates, emit_indexing, emit_jumps, emit_modules, emit_properties, emit_strings,
    emit_misc_ops, CodegenCtx,
};

#[allow(unreachable_patterns)]
pub fn compile_proto(
    proto: &FunctionProto,
    helpers: crate::JitHelpers,
) -> Result<JitBuffer, String> {
    let code = &proto.chunk.code;
    let mut ip = 0;
    while ip < code.len() {
        let raw_op = code[ip];
        ip += 1;
        let op =
            OpCode::from_u8(raw_op as u8).ok_or_else(|| format!("Unknown opcode: {}", raw_op))?;

        match op {
            OpCode::Return => {
                ip += 1;
            }
            OpCode::LoadNull
            | OpCode::LoadTrue
            | OpCode::LoadFalse
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne
            | OpCode::PopTry
            | OpCode::Nop => {}
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
            OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
                ip += 2;
            }
            OpCode::Move => {
                ip += 1;
            }
            OpCode::AddImm | OpCode::SubImm => {
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
            OpCode::Jump | OpCode::Loop | OpCode::JumpIfFalse | OpCode::JumpIfTrue => {
                ip += 2;
            }
            OpCode::GetProperty | OpCode::SetProperty | OpCode::Call | OpCode::BuildArray => {
                ip += 2;
            }
            OpCode::CallMethod | OpCode::InvokeVirtual | OpCode::Try => {
                ip += 3;
            }
            OpCode::BuildStr => {
                let w1 = code[ip];
                let count = (w1 >> 8) as usize;
                ip += 1 + count;
            }
            OpCode::LoadModuleSlot => {
                ip += 2;
            }
            OpCode::BuildObjectWithShape => {
                ip += 2;
            }
            OpCode::InvokeRuntimeStatic => {
                let _w1 = code[ip];
                ip += 1;
                let method_idx = code[ip] as usize;
                ip += 1;
                ip += 2;
                match proto.chunk.constants.get(method_idx) {
                    Some(varn_types::chunk::PoolEntry::Literal(
                        varn_types::chunk::Literal::Str(s),
                    )) if s.as_ref() == "__range__" => {}
                    _ => {
                        return Err(format!(
                            "JIT Bailout: InvokeRuntimeStatic with unsupported method"
                        ));
                    }
                }
            }
            OpCode::AssertNotNull
            | OpCode::CloseUpvalue
            | OpCode::GetEnumTag
            | OpCode::IsArray
            | OpCode::WrapSpread
            | OpCode::ObjectKeys
            | OpCode::ObjectMerge
            | OpCode::In
            | OpCode::GetSuper
            | OpCode::Inherit
            | OpCode::LoadModule
            | OpCode::StoreModuleSlot
            | OpCode::Spawn
            | OpCode::Throw
            | OpCode::Await
            | OpCode::Yield => {
                ip += 1;
            }
            OpCode::GetFixedField
            | OpCode::SetFixedField
            | OpCode::GetPropertyMaybe
            | OpCode::GetSymbol
            | OpCode::BindMethod
            | OpCode::DefineGlobal
            | OpCode::StoreGlobal
            | OpCode::DeclareField
            | OpCode::MakeClass
            | OpCode::Method
            | OpCode::DefineStatic
            | OpCode::DefineGetter
            | OpCode::DefineSetter
            | OpCode::DefineStaticGetter
            | OpCode::DefineStaticSetter
            | OpCode::MakeEnumVariant
            | OpCode::CallSpread => {
                ip += 2;
            }
            OpCode::BuildObject => {
                let w1 = code[ip];
                let count = (w1 & 0xFF) as usize;
                ip += 1 + count * 2;
            }
            OpCode::ObjectRest => {
                let w2 = code[ip + 1];
                let skip_count = (w2 >> 8) as usize;
                ip += 2 + skip_count;
            }
            _ => {
                return Err(format!(
                    "JIT Bailout: Opcode '{:?}' is not supported in the JIT compiler",
                    op
                ));
            }
        }
    }

    let regmap = RegMap::from_bytecode(code);

    let asm = Assembler::new();
    let code_len = code.len();

    let ip_to_asm_pos = vec![0usize; code_len + 1];
    let jump_patches = Vec::new();

    let mut cctx = CodegenCtx {
        asm,
        code,
        ip: 0,
        regmap,
        jump_patches,
        ip_to_asm_pos,
        proto,
        helpers: &helpers,
    };

    emit_prologue(&mut cctx.asm, &cctx.regmap);

    for (&vreg, &phys) in cctx.regmap.map_iter() {
        emit_load_reg(&mut cctx.asm, phys, vreg);
    }

    while cctx.ip < code_len {
        cctx.ip_to_asm_pos[cctx.ip] = cctx.asm.current_offset();

        let raw_op = cctx.code[cctx.ip];
        cctx.ip += 1;
        let first_reg = (raw_op >> 8) as usize;
        let op = OpCode::from_u8(raw_op as u8).unwrap();
        match op {
            OpCode::LoadNull
            | OpCode::LoadTrue
            | OpCode::LoadFalse
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne
            | OpCode::LoadInt
            | OpCode::LoadConst
            | OpCode::Move
            | OpCode::Return => emit_immediates(&mut cctx, op, first_reg)?,

            OpCode::LoadGlobalIdx
            | OpCode::StoreGlobalIdx
            | OpCode::DefineGlobalIdx
            | OpCode::LoadGlobal => emit_globals(&mut cctx, op, first_reg)?,

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
            | OpCode::Ushr
            | OpCode::AddImm
            | OpCode::SubImm
            | OpCode::AddInt
            | OpCode::SubInt
            | OpCode::MulInt
            | OpCode::ToString
            | OpCode::IsNull
            | OpCode::Not
            | OpCode::Negate => emit_arith(&mut cctx, op, first_reg)?,

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
            | OpCode::LtInt
            | OpCode::GtInt
            | OpCode::LteInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt => emit_compare(&mut cctx, op, first_reg)?,

            OpCode::Jump | OpCode::Loop | OpCode::JumpIfFalse | OpCode::JumpIfTrue => {
                emit_jumps(&mut cctx, op, first_reg)?
            }

            OpCode::LoadUpvalue | OpCode::StoreUpvalue | OpCode::MakeClosure => {
                emit_closures(&mut cctx, op, first_reg)?
            }

            OpCode::GetProperty | OpCode::SetProperty => emit_properties(&mut cctx, op, first_reg)?,

            OpCode::Call | OpCode::CallMethod | OpCode::InvokeVirtual => {
                emit_calls(&mut cctx, op, first_reg)?
            }

            OpCode::BuildArray
            | OpCode::ArrayLength
            | OpCode::ArrayPush
            | OpCode::ArrayPop
            | OpCode::ArrayExtend
            | OpCode::BuildStr => emit_arrays(&mut cctx, op, first_reg)?,

            OpCode::StrConcat | OpCode::StrSlice | OpCode::StrLength => {
                emit_strings(&mut cctx, op, first_reg)?
            }

            OpCode::GetIndex | OpCode::SetIndex | OpCode::Typeof | OpCode::Instanceof => {
                emit_indexing(&mut cctx, op, first_reg)?
            }

            OpCode::LoadModuleSlot | OpCode::BuildObjectWithShape | OpCode::InvokeRuntimeStatic => {
                emit_modules(&mut cctx, op, first_reg)?
            }

            OpCode::AssertNotNull
            | OpCode::CloseUpvalue
            | OpCode::GetEnumTag
            | OpCode::IsArray
            | OpCode::WrapSpread
            | OpCode::ObjectKeys
            | OpCode::ObjectMerge
            | OpCode::In
            | OpCode::GetFixedField
            | OpCode::SetFixedField
            | OpCode::GetPropertyMaybe
            | OpCode::GetSuper
            | OpCode::GetSymbol
            | OpCode::BindMethod
            | OpCode::DefineGlobal
            | OpCode::StoreGlobal
            | OpCode::DeclareField
            | OpCode::MakeClass
            | OpCode::Inherit
            | OpCode::Method
            | OpCode::DefineStatic
            | OpCode::DefineGetter
            | OpCode::DefineSetter
            | OpCode::DefineStaticGetter
            | OpCode::DefineStaticSetter
            | OpCode::BuildObject
            | OpCode::ObjectRest
            | OpCode::MakeEnumVariant
            | OpCode::CallSpread
            | OpCode::LoadModule
            | OpCode::StoreModuleSlot
            | OpCode::Spawn
            | OpCode::Try
            | OpCode::PopTry
            | OpCode::Throw
            | OpCode::Await
            | OpCode::Yield => emit_misc_ops(&mut cctx, op, first_reg)?,

            OpCode::Nop => {}

            _ => unreachable!(),
        }
    }

    cctx.ip_to_asm_pos[code_len] = cctx.asm.current_offset();

    for patch in &cctx.jump_patches {
        let target_asm_pos = cctx.ip_to_asm_pos[patch.target_bytecode_ip];
        let displacement = (target_asm_pos as isize - (patch.patch_pos + 4) as isize) as i32;
        cctx.asm.patch_u32(patch.patch_pos, displacement as u32);
    }

    let native_bytes = cctx.asm.into_bytes();
    let mut jit_buf = JitBuffer::new(native_bytes.len())?;
    jit_buf.as_mut_slice()[..native_bytes.len()].copy_from_slice(&native_bytes);
    jit_buf.make_executable()?;

    Ok(jit_buf)
}
