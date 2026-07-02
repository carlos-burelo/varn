use varn_core::OpCode;
use varn_types::FunctionProto;

use crate::assembler::Assembler;
use crate::mem::JitBuffer;
use crate::regalloc::{emit_prologue, RegMap};

use crate::codegen::{
    emit_arith, emit_arrays, emit_calls, emit_closures, emit_compare, emit_globals,
    emit_immediates, emit_indexing, emit_jumps, emit_misc_ops, emit_modules, emit_properties,
    emit_strings, CodegenCtx,
};

#[allow(unreachable_patterns)]
pub fn compile_proto(
    proto: &FunctionProto,
    constants: &[varn_types::VmValue],
    helpers: crate::JitHelpers,
) -> Result<JitBuffer, String> {
    let code = &proto.chunk.code;
    // Set when the function materializes any closure: such a closure may capture
    // one of this function's locals by reference, aliasing its stack slot into a
    // heap upvalue cell. That breaks the "immediate locals live only in a
    // callee-saved register across a call" invariant, so it disables spill
    // elision around calls.
    let mut creates_closures = false;
    // Walk the instruction stream with the shared decoder. Instruction
    // shapes live ONLY in varn_types::bytecode::decode; whether an opcode
    // is JIT-supported is decided by the codegen dispatch below, which
    // bails out with Err for anything it cannot emit. There is no separate
    // supported-opcode list to fall out of sync.
    let mut ip = 0;
    while ip < code.len() {
        let op = OpCode::from_u16(code[ip])
            .ok_or_else(|| format!("Unknown opcode: {}", code[ip]))?;
        if op == OpCode::MakeClosure {
            creates_closures = true;
        }
        let info = varn_types::bytecode::decode(code, ip, &proto.chunk.constants)
            .ok_or_else(|| format!("Undecodable instruction at ip {}", ip))?;
        ip += info.len;
    }

    let mut asm = Assembler::new();
    let regmap = RegMap::from_bytecode(code, &proto.chunk.constants);
    emit_prologue(&mut asm, &regmap, proto, &helpers);
    crate::regalloc::emit_reload_all(&mut asm, &regmap);

    let is_pure = proto.upvalue_count == 0 && !proto.is_async && !proto.is_generator;
    let safe_int_call_opt = is_pure && !creates_closures;

    let code_len = code.len();
    let mut cctx = CodegenCtx {
        asm,
        code,
        ip: 0,
        regmap,
        jump_patches: Vec::new(),
        ip_to_asm_pos: vec![0; code_len + 1],
        proto,
        helpers: &helpers,
        constants,
        safe_int_call_opt,
    };

    while cctx.ip < code_len {
        let raw_op = cctx.code[cctx.ip];
        cctx.ip_to_asm_pos[cctx.ip] = cctx.asm.current_offset();
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
            | OpCode::ModFloat
            | OpCode::PowFloat
            | OpCode::DivInt
            | OpCode::ModInt
            | OpCode::PowInt
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

            OpCode::LoadUpvalue
            | OpCode::StoreUpvalue
            | OpCode::MakeClosure
            | OpCode::LoadStaticFn => emit_closures(&mut cctx, op, first_reg)?,

            OpCode::GetProperty | OpCode::SetProperty => emit_properties(&mut cctx, op, first_reg)?,

            OpCode::Call | OpCode::CallSelf | OpCode::CallMethod | OpCode::InvokeVirtual => {
                emit_calls(&mut cctx, op, first_reg)?
            }

            OpCode::BuildArray
            | OpCode::ArrayLength
            | OpCode::ArrayPush
            | OpCode::ArrayPop
            | OpCode::ArrayExtend
            | OpCode::BuildStr => emit_arrays(&mut cctx, op, first_reg)?,

            OpCode::Intrinsic | OpCode::CallNativeOp => emit_calls(&mut cctx, op, first_reg)?,

            OpCode::StrConcat | OpCode::StrSlice | OpCode::StrLength => {
                emit_strings(&mut cctx, op, first_reg)?
            }

            OpCode::GetIndex
            | OpCode::ArrayGetIndex
            | OpCode::SetIndex
            | OpCode::ArraySetIndex
            | OpCode::Typeof
            | OpCode::Instanceof => emit_indexing(&mut cctx, op, first_reg)?,

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

            // Codegen is the single authority on JIT support: anything
            // without an emit arm bails the whole function back to the
            // interpreter.
            _ => {
                return Err(format!(
                    "JIT Bailout: Opcode '{:?}' is not supported in the JIT compiler",
                    op
                ));
            }
        }
    }

    cctx.ip_to_asm_pos[code_len] = cctx.asm.current_offset();

    for patch in &cctx.jump_patches {
        let target_asm_pos = cctx.ip_to_asm_pos[patch.target_bytecode_ip];
        let displacement = (target_asm_pos as isize - (patch.patch_pos + 4) as isize) as i32;
        cctx.asm.patch_u32(patch.patch_pos, displacement as u32);
    }

    crate::codegen::optimizer::optimize(&mut cctx.asm)?;
    let native_bytes = cctx.asm.into_bytes();
    let mut jit_buf = JitBuffer::new(native_bytes.len())?;
    jit_buf.as_mut_slice()[..native_bytes.len()].copy_from_slice(&native_bytes);
    jit_buf.make_executable()?;

    Ok(jit_buf)
}
