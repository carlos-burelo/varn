//! Bytecode instruction decoder — single source of truth for instruction
//! shapes: length in words, defined register, used registers and
//! call-argument windows. Every walker over `chunk.code` (backend register
//! allocation, slot-kind analysis, JIT pre-scan and JIT register mapping)
//! must advance with [`decode`]; hand-rolled `ip += N` width tables are how
//! the JIT once silently rejected whole functions and how `Spawn` shipped
//! with three incompatible encodings.

use varn_core::OpCode;

use crate::chunk::PoolEntry;

pub struct InstrInfo {
    /// Total instruction length in code words, including the opcode word.
    pub len: usize,

    /// Register defined (written) by this instruction, if any.
    pub def: Option<u8>,

    /// Registers read by this instruction.
    pub uses: Vec<u8>,

    /// `(arg_start, arg_count)` window for call-shaped instructions that
    /// require their arguments contiguous on the register file.
    pub call_args: Option<(u8, u8)>,

    /// Control-flow or otherwise unanalyzable instruction: walkers must
    /// treat every register as potentially live across it.
    pub opaque: bool,
}

impl InstrInfo {
    fn simple(len: usize, def: Option<u8>, uses: Vec<u8>) -> Self {
        Self {
            len,
            def,
            uses,
            call_args: None,
            opaque: false,
        }
    }
    fn opaque(len: usize) -> Self {
        Self {
            len,
            def: None,
            uses: vec![],
            call_args: None,
            opaque: true,
        }
    }
}

pub fn decode(code: &[u16], offset: usize, constants: &[PoolEntry]) -> Option<InstrInfo> {
    let op = OpCode::from_u16(*code.get(offset)?)?;

    let get = |off: usize| code.get(offset + off).copied().unwrap_or(0);
    let w0 = get(0);
    let w1 = get(1);
    let w2 = get(2);
    let w3 = get(3);
    let w4 = get(4);

    let dest0 = (w0 >> 8) as u8;
    let hi1 = (w1 >> 8) as u8;
    let lo1 = (w1 & 0xff) as u8;
    let hi2 = (w2 >> 8) as u8;
    let lo2 = (w2 & 0xff) as u8;
    let hi3 = (w3 >> 8) as u8;
    let lo3 = (w3 & 0xff) as u8;
    let hi4 = (w4 >> 8) as u8;
    let _lo4 = (w4 & 0xff) as u8;

    let s = InstrInfo::simple;
    let info = match op {
        OpCode::PopTry | OpCode::Nop => s(1, None, vec![]),
        OpCode::Intrinsic => {
            let arg_count = lo1 as usize;
            let mut uses = vec![];
            for i in 0..arg_count {
                uses.push(dest0.wrapping_add(i as u8));
            }
            InstrInfo {
                len: 2,
                def: Some(dest0),
                uses,
                call_args: Some((dest0, arg_count as u8)),
                opaque: false,
            }
        }
        OpCode::CallNativeOp => {
            // Operands: [op_id_const_idx][arg_count]. Receiver + args are
            // contiguous from `dest0` (call_base); `arg_count` includes the
            // receiver. Mirrors `Intrinsic` so regalloc keeps them contiguous.
            let total = lo2 as usize;
            let mut uses = vec![];
            for i in 0..total {
                uses.push(dest0.wrapping_add(i as u8));
            }
            InstrInfo {
                len: 3,
                def: Some(dest0),
                uses,
                call_args: Some((dest0, total as u8)),
                opaque: false,
            }
        }
        OpCode::LoadStaticFn => s(2, Some(dest0), vec![]),

        OpCode::LoadNull
        | OpCode::LoadTrue
        | OpCode::LoadFalse
        | OpCode::LoadIntZero
        | OpCode::LoadIntOne
        | OpCode::LoadIntMinusOne => s(1, Some(dest0), vec![]),
        OpCode::LoadUpvalue => s(2, Some(hi1), vec![]),

        OpCode::Move
        | OpCode::Negate
        | OpCode::Not
        | OpCode::ToString
        | OpCode::IsNull
        | OpCode::IsArray
        | OpCode::Typeof
        | OpCode::WrapSpread
        | OpCode::ArrayLength
        | OpCode::ArrayPop
        | OpCode::StrLength
        | OpCode::GetEnumTag
        | OpCode::Await
        | OpCode::ObjectKeys => s(2, Some(dest0), vec![hi1]),

        // Spawn encodes like Await: dest in the opcode word, task register
        // in the operand word. (It historically had three disagreeing
        // encodings across emitter, dispatch and JIT — see git history.)
        OpCode::Spawn => s(2, Some(dest0), vec![hi1]),

        OpCode::ArrayPush | OpCode::Inherit => s(2, None, vec![hi1, lo1]),
        OpCode::Yield | OpCode::Return => s(2, None, vec![lo1]),
        OpCode::Throw => s(2, None, vec![hi1]),
        OpCode::StoreUpvalue => s(2, None, vec![lo1]),
        OpCode::CloseUpvalue => s(2, None, vec![hi1]),

        OpCode::LoadModule => s(2, Some(dest0), vec![]),

        OpCode::StoreModuleSlot => s(2, None, vec![dest0]),

        OpCode::LoadModuleSlot => s(3, Some(dest0), vec![hi1]),

        OpCode::MakeClosure => {
            let uv_count = lo1 as usize;
            let dest = hi1;
            let mut captured_locals = vec![];
            for i in 0..uv_count {
                let desc = get(3 + i);
                let is_local = (desc >> 8) as u8;
                let local_idx = (desc & 0xff) as u8;
                if is_local == 1 {
                    captured_locals.push(local_idx);
                }
            }
            InstrInfo {
                len: 3 + uv_count,
                def: Some(dest),
                uses: captured_locals,
                call_args: None,
                opaque: false,
            }
        }

        OpCode::BuildObject => {
            let count = lo1 as usize;
            let dest = hi1;

            let mut uses = vec![];
            for i in 0..count {
                let pair_word = get(2 + i * 2 + 1);
                let val_reg = (pair_word >> 8) as u8;
                uses.push(val_reg);
            }
            InstrInfo {
                len: 2 + count * 2,
                def: Some(dest),
                uses,
                call_args: None,
                opaque: false,
            }
        }

        OpCode::BuildObjectWithShape => {
            let start = lo1 as usize;
            let shape_idx = w2 as usize;
            let count = match constants.get(shape_idx) {
                Some(PoolEntry::Shape(k)) => k.len(),
                _ => 0,
            };
            let dest = hi1;
            let mut uses = vec![];
            for i in 0..count {
                uses.push((start + i) as u8);
            }
            InstrInfo {
                len: 3,
                def: Some(dest),
                uses,
                call_args: None,
                opaque: false,
            }
        }

        OpCode::BuildArray => {
            let start = lo1 as usize;
            let count = hi2 as usize;
            let dest = hi1;
            let mut uses = vec![];
            for i in 0..count {
                uses.push((start + i) as u8);
            }
            InstrInfo {
                len: 3,
                def: Some(dest),
                uses,
                call_args: None,
                opaque: false,
            }
        }

        OpCode::GetIndex | OpCode::ArrayGetIndex => s(2, Some(dest0), vec![hi1, lo1]),

        OpCode::SetIndex | OpCode::ArraySetIndex => s(2, None, vec![dest0, hi1, lo1]),

        OpCode::ObjectMerge => s(2, Some(dest0), vec![hi1]),

        OpCode::ObjectRest => {
            let skip_count = hi2 as usize;
            s(3 + skip_count, Some(hi1), vec![lo1])
        }

        OpCode::Add
        | OpCode::Sub
        | OpCode::Mul
        | OpCode::Div
        | OpCode::Mod
        | OpCode::Pow
        | OpCode::BitAnd
        | OpCode::BitOr
        | OpCode::BitXor
        | OpCode::Shl
        | OpCode::Shr
        | OpCode::Ushr
        | OpCode::Eq
        | OpCode::Neq
        | OpCode::Lt
        | OpCode::Lte
        | OpCode::Gt
        | OpCode::Gte
        | OpCode::Instanceof
        | OpCode::In
        | OpCode::StrConcat
        | OpCode::StrSlice
        | OpCode::AddInt
        | OpCode::SubInt
        | OpCode::MulInt
        | OpCode::DivInt
        | OpCode::ModInt
        | OpCode::PowInt
        | OpCode::LtInt
        | OpCode::GtInt
        | OpCode::LteInt
        | OpCode::GteInt
        | OpCode::EqInt
        | OpCode::NeqInt
        | OpCode::AddFloat
        | OpCode::SubFloat
        | OpCode::MulFloat
        | OpCode::DivFloat
        | OpCode::ModFloat
        | OpCode::PowFloat
        | OpCode::LtFloat
        | OpCode::GtFloat
        | OpCode::LteFloat
        | OpCode::GteFloat
        | OpCode::EqFloat
        | OpCode::NeqFloat => s(2, Some(dest0), vec![hi1, lo1]),

        OpCode::AddImm | OpCode::SubImm => s(2, Some(dest0), vec![hi1]),

        OpCode::LoadConst | OpCode::LoadInt | OpCode::LoadGlobal | OpCode::LoadGlobalIdx => {
            s(2, Some(dest0), vec![])
        }

        OpCode::StoreGlobal
        | OpCode::DefineGlobal
        | OpCode::StoreGlobalIdx
        | OpCode::DefineGlobalIdx => s(3, None, vec![hi1]),

        OpCode::AssertNotNull => s(2, None, vec![hi1]),

        OpCode::MakeClass => s(3, Some(dest0), vec![hi1]),

        OpCode::DeclareField => s(3, None, vec![hi1]),

        OpCode::ArrayExtend => s(2, Some(dest0), vec![hi1]),

        OpCode::Jump | OpCode::Loop => InstrInfo::opaque(3),

        OpCode::JumpIfFalse | OpCode::JumpIfTrue => s(3, None, vec![dest0]),

        OpCode::Try => s(4, Some(hi1), vec![]),

        OpCode::InvokeRuntimeStatic => InstrInfo {
            len: 5,
            def: Some(hi1),
            uses: vec![lo3, hi4],
            call_args: None,
            opaque: false,
        },

        OpCode::GetProperty
        | OpCode::GetPropertyMaybe
        | OpCode::GetFixedField
        | OpCode::GetSymbol => s(3, Some(dest0), vec![hi1]),
        OpCode::SetProperty | OpCode::SetFixedField => s(3, None, vec![dest0, hi1]),

        OpCode::GetSuper => s(2, Some(dest0), vec![]),

        OpCode::BindMethod => s(3, Some(hi1), vec![lo1]),

        OpCode::Method
        | OpCode::DefineStatic
        | OpCode::DefineGetter
        | OpCode::DefineSetter
        | OpCode::DefineStaticGetter
        | OpCode::DefineStaticSetter => s(3, None, vec![hi1, lo1]),

        OpCode::MakeEnumVariant => s(3, Some(hi1), vec![lo1]),

        OpCode::InvokeVirtual => {
            let dest = hi1;
            let this_reg = lo1;
            let argc = hi3;
            let arg_start = lo3;
            let mut uses = vec![this_reg];
            for i in 0..argc {
                uses.push(arg_start.wrapping_add(i));
            }
            InstrInfo {
                len: 4,
                def: Some(dest),
                uses,
                call_args: Some((arg_start, argc)),
                opaque: false,
            }
        }

        OpCode::Call | OpCode::CallSpread | OpCode::CallSelf => {
            let dest = hi1;
            let fn_reg = lo1;
            let argc = hi2;
            let arg_start = lo2;
            let mut uses = if op == OpCode::CallSelf {
                Vec::new()
            } else {
                vec![fn_reg]
            };
            for i in 0..argc {
                uses.push(arg_start.wrapping_add(i));
            }
            InstrInfo {
                len: 3,
                def: Some(dest),
                uses,
                call_args: Some((arg_start, argc)),
                opaque: false,
            }
        }

        OpCode::BuildStr => {
            let count = hi1 as usize;
            let dest = dest0;
            let mut uses = vec![];
            for i in 0..count {
                let w = get(2 + i);
                uses.push((w >> 8) as u8);
            }
            InstrInfo {
                len: 2 + count,
                def: Some(dest),
                uses,
                call_args: None,
                opaque: false,
            }
        }

        OpCode::CallMethod => {
            let mut uses = vec![lo1];
            for i in 0..hi3 {
                uses.push(lo3.wrapping_add(i));
            }
            InstrInfo {
                len: 4,
                def: Some(hi1),
                uses,
                call_args: Some((lo3, hi3)),
                opaque: false,
            }
        }
    };

    Some(info)
}
