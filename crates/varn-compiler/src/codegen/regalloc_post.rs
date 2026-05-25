use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use varn_core::OpCode;
use varn_types::FunctionProto;

thread_local! {
    pub static OPTIMIZE_TIME: Cell<Duration> = Cell::new(Duration::ZERO);
    pub static OPTIMIZE_ENABLED: Cell<bool> = Cell::new(false);
}

use super::liveness::{LiveRange, LivenessAnalyzer};

struct InstrInfo {
    len: usize,

    def: Option<u8>,

    uses: Vec<u8>,

    call_args: Option<(u8, u8)>,

    opaque: bool,
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

fn decode(code: &[u16], offset: usize) -> Option<InstrInfo> {
    let op = OpCode::from_u16(*code.get(offset)?)?;

    let get = |off: usize| code.get(offset + off).copied().unwrap_or(0);
    let w0 = get(0);
    let w1 = get(1);
    let w2 = get(2);
    let w3 = get(3);
    let w4 = get(4);

    // hi byte of word[0] is the dest register (packed as pack_op(op, dest)).
    let dest0 = (w0 >> 8) as u8;
    let hi1 = (w1 >> 8) as u8;
    let lo1 = (w1 & 0xff) as u8;
    let hi2 = (w2 >> 8) as u8;
    let lo2 = (w2 & 0xff) as u8;
    let hi3 = (w3 >> 8) as u8;
    let lo3 = (w3 & 0xff) as u8;
    let _hi4 = (w4 >> 8) as u8;

    let s = InstrInfo::simple;
    let info = match op {
        OpCode::PopTry | OpCode::Nop => s(1, None, vec![]),

        // 1-word opcodes: dest is in the high byte of word[0] (dest0), no second word.
        OpCode::LoadNull
        | OpCode::LoadTrue
        | OpCode::LoadFalse
        | OpCode::LoadIntZero
        | OpCode::LoadIntOne
        | OpCode::LoadIntMinusOne => s(1, Some(dest0), vec![]),
        OpCode::LoadUpvalue => s(2, Some(hi1), vec![]),

        // 2-word emit_rr: [pack_op(op,dest), pack(src,0)]
        // dest = dest0 (word[0] high), src = hi1 (word[1] high), lo1 = 0 (padding)
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

        OpCode::ArrayPush | OpCode::Inherit => s(2, None, vec![hi1, lo1]),
        OpCode::Yield | OpCode::Return | OpCode::Throw => s(2, None, vec![lo1]),
        OpCode::StoreUpvalue => s(2, None, vec![lo1]),
        OpCode::CloseUpvalue => s(2, None, vec![lo1]),

        // 2-word emit_rc: [pack_op(op,dest), const_idx]
        OpCode::LoadModule => s(2, Some(dest0), vec![]),

        // emit_rc: [pack_op(op,val_reg), slot_idx]
        OpCode::StoreModuleSlot => s(2, None, vec![dest0]),

        // emit_rrc: [pack_op(op,dest), pack(src,0), const_idx]
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

        OpCode::BuildObjectWithShape => s(3, Some(hi1), vec![lo1]),

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

        // 2-word emit_rrr: [pack_op(op,dest), pack(src1,src2)]
        // GetIndex: dest=dest0, obj=hi1, key=lo1
        OpCode::GetIndex => s(2, Some(dest0), vec![hi1, lo1]),

        // SetIndex: emit_rrr(op, obj, key, val) → dest0=obj (used not def), hi1=key, lo1=val
        OpCode::SetIndex => s(2, None, vec![dest0, hi1, lo1]),

        OpCode::ObjectRest => {
            let skip_count = hi2 as usize;
            s(3 + skip_count, Some(hi1), vec![lo1])
        }

        // 2-word: [pack_op(op,dest), pack(src1,src2)]
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
        | OpCode::LtFloat
        | OpCode::GtFloat
        | OpCode::LteFloat
        | OpCode::GteFloat
        | OpCode::EqFloat
        | OpCode::NeqFloat => s(2, Some(dest0), vec![hi1, lo1]),

        OpCode::AddImm | OpCode::SubImm => s(2, Some(dest0), vec![hi1]),

        // 2-word: [pack_op(op,dest), const_idx]
        OpCode::LoadConst | OpCode::LoadInt | OpCode::LoadGlobal | OpCode::LoadGlobalIdx => {
            s(2, Some(dest0), vec![])
        }

        // 3-word: [pack_op(op,dest), pack(src,0), const_idx]
        OpCode::StoreGlobal
        | OpCode::DefineGlobal
        | OpCode::StoreGlobalIdx
        | OpCode::DefineGlobalIdx => s(3, None, vec![hi1]),

        // AssertNotNull: emit1(op, pack(0,r)) — 2 words: [op, pack(0,r)]
        OpCode::AssertNotNull => s(2, None, vec![lo1]),

        // MakeClass: emit_rrc(op,dest,src,const_idx) → [pack_op(op,dest), pack(src,0), const_idx]
        // dest=dest0, src=hi1, lo1=0 (padding), word[2]=name_idx
        OpCode::MakeClass => s(3, Some(dest0), vec![hi1]),

        // Spawn: bare emit → 1 word
        OpCode::Spawn => InstrInfo::opaque(1),

        // DeclareField: bare emit → 1 word
        OpCode::DeclareField => InstrInfo::opaque(1),

        // ArrayExtend: [pack_op(op,dest), pack(src,0)]
        OpCode::ArrayExtend => s(2, Some(dest0), vec![hi1]),

        OpCode::Jump | OpCode::Loop => InstrInfo::opaque(3),

        OpCode::JumpIfFalse | OpCode::JumpIfTrue => s(3, None, vec![dest0]),

        OpCode::Try => InstrInfo::opaque(4),

        OpCode::InvokeRuntimeStatic => InstrInfo::opaque(5),

        // GetProperty: word[0]=(op|dest), word[1]=(src|cs), word[2]=name_idx.
        OpCode::GetProperty | OpCode::GetPropertyMaybe => s(3, Some(dest0), vec![hi1]),

        // SetProperty: word[0]=(op|obj), word[1]=(val|cs), word[2]=name_idx.
        OpCode::SetProperty => s(3, None, vec![dest0, hi1]),

        // GetFixedField: [pack_op(op,dest), pack(obj,0), slot_idx] — dest=dest0, obj=hi1
        // GetSymbol: [pack_op(op,dest), pack(obj,cs), sym_idx] — dest=dest0, obj=hi1
        OpCode::GetFixedField | OpCode::GetSymbol => s(3, Some(dest0), vec![hi1]),

        // SetFixedField: [pack_op(op,obj), pack(val,0), slot_idx] — obj=dest0, val=hi1
        OpCode::SetFixedField => s(3, None, vec![dest0, hi1]),

        // GetSuper: emit_rc(op,dest,name_idx) → [pack_op(op,dest), name_idx] — dest=dest0, no reg src
        OpCode::GetSuper => s(2, Some(dest0), vec![]),

        // BindMethod: emit(op) then write(pack(dest,obj)) then write(name_idx)
        // word[0]=opcode only, word[1]=pack(dest,obj), word[2]=name_idx
        OpCode::BindMethod => s(3, Some(hi1), vec![lo1]),

        // Method/DefineStatic/...: 3-word class member definitions
        OpCode::Method
        | OpCode::DefineStatic
        | OpCode::DefineGetter
        | OpCode::DefineSetter
        | OpCode::DefineStaticGetter
        | OpCode::DefineStaticSetter => InstrInfo::opaque(3),

        // MakeEnumVariant: emit(op), write(pack(variant_reg,val_reg)), write(name_idx)
        // word[0]=opcode only, word[1]=pack(variant_reg,val_reg), word[2]=name_idx
        OpCode::MakeEnumVariant => s(3, Some(hi1), vec![lo1]),

        // Call/CallSpread/InvokeVirtual: [pack_op(op,0), pack(dest,fn_reg), pack(argc,arg_start)]
        OpCode::Call | OpCode::CallSpread | OpCode::InvokeVirtual => {
            let dest = hi1;
            let fn_reg = lo1;
            let argc = hi2;
            let arg_start = lo2;
            let mut uses = vec![fn_reg];
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

        // BuildStr: [pack_op(op,dest), pack(count,0), pack(reg0,0), pack(reg1,0), ...]
        // word[0]=pack_op(op,dest), word[1]=pack(count,0), word[2+i]=pack(reg_i,0)
        OpCode::BuildStr => {
            let count = hi1 as usize; // count is in word[1] high byte
            let dest = dest0; // dest is in word[0] high byte
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

        // CallMethod: word[0]=(op|cs), word[1]=(dest|obj), word[2]=name_idx, word[3]=(argc|arg_start).
        // arg_start=lo3, argc=hi3; args must stay contiguous so track call_args.
        OpCode::CallMethod => {
            let mut uses = vec![lo1];
            for i in 0..hi3 {
                uses.push(lo3.wrapping_add(i));
            }
            InstrInfo {
                len: 4,
                def: Some(hi1),
                uses,
                call_args: if hi3 > 1 { Some((lo3, hi3)) } else { None },
                opaque: false,
            }
        }

        _ => InstrInfo::opaque(1),
    };

    Some(info)
}

#[allow(dead_code)]
struct ScanResult {
    defs: HashMap<u8, usize>,

    uses: HashMap<u8, Vec<usize>>,

    call_arg_ranges: Vec<(u8, u8)>,

    pinned: std::collections::HashSet<u8>,
}

fn scan_bytecode(code: &[u16]) -> ScanResult {
    let mut defs: HashMap<u8, usize> = HashMap::new();
    let mut uses: HashMap<u8, Vec<usize>> = HashMap::new();
    let mut call_arg_ranges: Vec<(u8, u8)> = Vec::new();
    let pinned: std::collections::HashSet<u8> = std::collections::HashSet::new();

    let mut offset = 0;
    let mut instr_idx = 0usize;

    while offset < code.len() {
        let info = match decode(code, offset) {
            Some(i) => i,
            None => break,
        };

        if info.opaque {
            offset += info.len;
            instr_idx += 1;
            continue;
        }

        if let Some(def_reg) = info.def {
            defs.entry(def_reg).or_insert(instr_idx);
        }
        for &use_reg in &info.uses {
            uses.entry(use_reg).or_insert_with(Vec::new).push(instr_idx);
        }
        if let Some(args) = info.call_args {
            call_arg_ranges.push(args);
        }

        offset += info.len;
        instr_idx += 1;
    }

    ScanResult {
        defs,
        uses,
        call_arg_ranges,
        pinned,
    }
}

fn color_with_base(ranges: &[LiveRange], base: u8) -> HashMap<u8, u8> {
    let mut coloring: HashMap<u8, u8> = HashMap::new();

    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|r| r.start);

    for range in &sorted {
        let reg = range.vreg as u8;
        let neighbor_colors: std::collections::HashSet<u8> = range
            .interference
            .iter()
            .filter_map(|&n| coloring.get(&(n as u8)).copied())
            .collect();

        let color = (base..)
            .find(|c| !neighbor_colors.contains(c))
            .unwrap_or(base);
        coloring.insert(reg, color);
    }

    coloring
}

fn verify_call_constraints(code: &[u16], mapping: &HashMap<u8, u8>) -> bool {
    let mut offset = 0;
    while offset < code.len() {
        let info = match decode(code, offset) {
            Some(i) => i,
            None => break,
        };
        if let Some((arg_start, arg_count)) = info.call_args {
            let mapped_start = match mapping.get(&arg_start) {
                Some(&m) => m,
                None => arg_start,
            };
            for i in 1..arg_count {
                let orig = arg_start.wrapping_add(i);
                let mapped = mapping.get(&orig).copied().unwrap_or(orig);
                if mapped != mapped_start.wrapping_add(i) {
                    return false;
                }
            }
        }
        offset += info.len;
    }
    true
}

fn remap_bytecode(code: &mut Vec<u16>, mapping: &HashMap<u8, u8>) {
    fn m(mapping: &HashMap<u8, u8>, r: u8) -> u8 {
        mapping.get(&r).copied().unwrap_or(r)
    }

    let mut offset = 0;
    while offset < code.len() {
        let info = match decode(code, offset) {
            Some(i) => i,
            None => break,
        };

        if !info.opaque {
            let op = match OpCode::from_u16(code[offset]) {
                Some(op) => op,
                None => {
                    offset += info.len;
                    continue;
                }
            };

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
            let _hi4 = (w4 >> 8) as u8;

            #[inline(always)]
            fn pack(a: u8, b: u8) -> u16 {
                ((a as u16) << 8) | (b as u16)
            }

            #[inline(always)]
            fn pack_op(op: OpCode, reg: u8) -> u16 {
                ((reg as u16) << 8) | (op as u8 as u16)
            }

            if OpCode::from_u16(code[offset]) == Some(OpCode::Try) {
                let w1 = code.get(offset + 1).copied().unwrap_or(0);
                let old_err_reg = (w1 >> 8) as u8;
                let new_err_reg = m(mapping, old_err_reg);
                code[offset + 1] = ((new_err_reg as u16) << 8) | 0;
            }

            match op {
                OpCode::PopTry | OpCode::Nop => {}

                OpCode::LoadNull
                | OpCode::LoadTrue
                | OpCode::LoadFalse
                | OpCode::LoadIntZero
                | OpCode::LoadIntOne
                | OpCode::LoadIntMinusOne => {
                    // dest is in the high byte of word[0]; no second word to touch.
                    let op_byte = code[offset] & 0xFF;
                    code[offset] = ((m(mapping, dest0) as u16) << 8) | op_byte;
                }
                OpCode::LoadUpvalue => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }

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
                | OpCode::ObjectKeys => {
                    // emit_rr: [pack_op(op,dest), pack(src,0)]
                    // dest=dest0, src=hi1, lo1=0 (padding, not a register)
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), 0);
                }

                OpCode::ArrayPush | OpCode::Inherit => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }
                OpCode::Yield | OpCode::Return | OpCode::Throw => {
                    code[offset + 1] = pack(hi1, m(mapping, lo1));
                }
                OpCode::StoreUpvalue => {
                    code[offset + 1] = pack(hi1, m(mapping, lo1));
                }
                OpCode::CloseUpvalue => {
                    code[offset + 1] = pack(hi1, m(mapping, lo1));
                }
                OpCode::AssertNotNull => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }
                OpCode::ObjectMerge => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }

                OpCode::Jump | OpCode::Loop => {}

                // emit_rc: [pack_op(op,dest), const_idx] — word[1] is a raw index, not a register
                OpCode::LoadConst
                | OpCode::LoadInt
                | OpCode::LoadGlobal
                | OpCode::LoadGlobalIdx => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    // word[1] = const_idx, do not touch
                }
                // emit_rrc: [pack_op(op,dest), pack(src,0), const_idx]
                // dest=dest0, src=hi1, lo1=0 (padding)
                OpCode::MakeClass => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), 0);
                }
                // emit_rc: [pack_op(op,dest), name_idx] — no src register
                OpCode::GetSuper => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    // word[1] = name_idx, do not touch
                }

                // emit_rrc(op,0,src,name_idx): [pack_op(op,0), pack(src,0), name_idx]
                // src = hi1, lo1 = 0 (padding), word[2] = name_idx (not a register)
                OpCode::StoreGlobal
                | OpCode::DefineGlobal
                | OpCode::StoreGlobalIdx
                | OpCode::DefineGlobalIdx => {
                    code[offset + 1] = pack(m(mapping, hi1), 0);
                }

                // emit_rrr: [pack_op(op,dest), pack(src1,src2)] — 2 words only
                // dest=dest0, src1=hi1, src2=lo1
                OpCode::Add
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
                | OpCode::AddInt
                | OpCode::SubInt
                | OpCode::MulInt
                | OpCode::DivInt
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
                | OpCode::LtFloat
                | OpCode::GtFloat
                | OpCode::LteFloat
                | OpCode::GteFloat
                | OpCode::EqFloat
                | OpCode::NeqFloat => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }
                OpCode::ArrayExtend => {
                    // emit_rrr: dest=dest0, src=hi1
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), 0);
                }

                // 3-word with dest=dest0, src=hi1: remap both word[0] and word[1]
                OpCode::GetPropertyMaybe
                | OpCode::GetFixedField
                | OpCode::GetSymbol
                | OpCode::LoadModuleSlot => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }
                // BindMethod: word[0]=opcode only, word[1]=pack(dest,obj), word[2]=name_idx
                OpCode::BindMethod => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }
                // MakeEnumVariant: word[0]=opcode only, word[1]=pack(variant_reg,val_reg)
                OpCode::MakeEnumVariant => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }
                // SetFixedField: word[0]=pack_op(op,obj), word[1]=pack(val,0)
                // obj=dest0, val=hi1
                OpCode::SetFixedField => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), 0);
                }
                OpCode::DeclareField => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }
                OpCode::Method
                | OpCode::DefineStatic
                | OpCode::DefineGetter
                | OpCode::DefineSetter
                | OpCode::DefineStaticGetter
                | OpCode::DefineStaticSetter => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }

                OpCode::JumpIfFalse | OpCode::JumpIfTrue => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                }

                OpCode::GetProperty => {
                    // dest in word[0] hi byte, src in word[1] hi byte.
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }

                OpCode::SetProperty => {
                    // obj in word[0] hi byte, val in word[1] hi byte.
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }

                OpCode::Call => {
                    let arg_count = hi2;
                    let arg_start = lo2;
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 2] = pack(arg_count, m(mapping, arg_start));
                }

                OpCode::CallSpread => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 2] = pack(hi2, m(mapping, lo2));
                }

                OpCode::InvokeVirtual => {
                    let arg_start = lo2;
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 3] = pack(hi2, m(mapping, arg_start));
                }

                OpCode::CallMethod => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 3] = pack(hi3, m(mapping, lo3));
                }

                // emit_rc: [pack_op(op,dest), const_idx] — word[1] is raw index
                OpCode::LoadModule | OpCode::StoreModuleSlot => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    // word[1] = const_idx, do not touch
                }

                OpCode::MakeClosure => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                    let uv_count = lo1 as usize;
                    for i in 0..uv_count {
                        let desc_off = offset + 3 + i;
                        if desc_off < code.len() {
                            let desc = code[desc_off];
                            let is_local = (desc >> 8) as u8;
                            let local_idx = (desc & 0xff) as u8;
                            if is_local == 1 {
                                code[desc_off] = pack(is_local, m(mapping, local_idx));
                            }
                        }
                    }
                }

                OpCode::BuildObject => {
                    let count = lo1 as usize;
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                    for i in 0..count {
                        let w_off = offset + 2 + i * 2 + 1;
                        if w_off < code.len() {
                            let pair = code[w_off];
                            let val_reg = (pair >> 8) as u8;
                            code[w_off] = pack(m(mapping, val_reg), pair as u8);
                        }
                    }
                }

                OpCode::BuildObjectWithShape => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }

                OpCode::BuildArray => {
                    let start = lo1 as usize;
                    let _count = hi2 as usize;
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, start as u8));
                }

                // emit_rrr 2-word: GetIndex dest=dest0, obj=hi1, key=lo1
                OpCode::GetIndex => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }
                // emit_rrr 2-word: SetIndex obj=dest0, key=hi1, val=lo1 (no def)
                OpCode::SetIndex => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }

                OpCode::ObjectRest => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }

                // [pack_op(op,dest), pack(src,imm)] — dest=dest0, src=hi1, lo1=imm (not a register)
                OpCode::AddImm | OpCode::SubImm => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), lo1); // preserve imm in lo byte
                }

                // [pack_op(op,dest), pack(count,0), pack(reg0,0), ...]
                // dest=dest0, count=hi1 (not a register), each word[2+i] hi = reg_i
                OpCode::BuildStr => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    let count = hi1 as usize;
                    for i in 0..count {
                        let w_off = offset + 2 + i;
                        if w_off < code.len() {
                            let reg = (code[w_off] >> 8) as u8;
                            code[w_off] = pack(m(mapping, reg), 0);
                        }
                    }
                }

                _ => {}
            }
        }

        offset += info.len;
    }
}

/// Returns `(loop_header_instr_idx, loop_instr_idx)` for every back-edge in the bytecode.
/// Both values are instruction indices (matching the units used by `scan_bytecode`).
/// `loop_header_instr_idx` is the instruction index of the loop body's first instruction.
/// `loop_instr_idx` is the instruction index of the `Loop` opcode itself.
fn collect_back_edges(code: &[u16]) -> Vec<(usize, usize)> {
    // First pass: build word_offset → instr_idx map.
    let mut word_to_instr: HashMap<usize, usize> = HashMap::new();
    let mut offset = 0usize;
    let mut instr_idx = 0usize;
    while offset < code.len() {
        word_to_instr.insert(offset, instr_idx);
        match decode(code, offset) {
            Some(info) => {
                offset += info.len;
                instr_idx += 1;
            }
            None => break,
        }
    }

    // Second pass: find Loop opcodes, compute targets in instr_idx units.
    let mut edges = Vec::new();
    let mut offset = 0usize;
    let mut instr_idx = 0usize;
    while offset < code.len() {
        if OpCode::from_u16(code[offset]) == Some(OpCode::Loop) {
            let hi = code.get(offset + 1).copied().unwrap_or(0) as usize;
            let lo = code.get(offset + 2).copied().unwrap_or(0) as usize;
            let back_offset = (hi << 16) | lo;
            let target_word = (offset + 3).saturating_sub(back_offset);
            let target_instr = word_to_instr.get(&target_word).copied().unwrap_or(0);
            edges.push((target_instr, instr_idx));
        }
        match decode(code, offset) {
            Some(info) => {
                offset += info.len;
                instr_idx += 1;
            }
            None => break,
        }
    }
    edges
}

pub fn optimize_function(proto: &mut FunctionProto) {
    let start = if OPTIMIZE_ENABLED.with(|e| e.get()) {
        Some(Instant::now())
    } else {
        None
    };

    optimize_function_inner(proto);

    if let Some(start) = start {
        let elapsed = start.elapsed();
        OPTIMIZE_TIME.with(|t| t.set(t.get() + elapsed));
    }
}

fn optimize_function_inner(proto: &mut FunctionProto) {
    // Async and generator functions save register state into a heap-allocated array.
    // Remapping registers would break the saved indices, so skip optimization for these.
    if proto.is_async || proto.is_generator {
        return;
    }

    let fixed_count = proto.arity + if proto.has_this { 1 } else { 0 };
    let base = fixed_count as u8;

    if proto.chunk.code.is_empty() {
        return;
    }

    let back_edges = collect_back_edges(&proto.chunk.code);
    let scan = scan_bytecode(&proto.chunk.code);

    let all_regs: Vec<u16> = scan
        .defs
        .keys()
        .filter(|&&r| r >= base)
        .map(|&r| r as u16)
        .collect();

    if all_regs.is_empty() {
        return;
    }

    let mut analyzer = LivenessAnalyzer::new();
    for (&reg, &def_pos) in &scan.defs {
        if reg >= base {
            analyzer.record_def(reg as u16, def_pos);
        }
    }
    for (&reg, use_positions) in &scan.uses {
        if reg >= base {
            for &pos in use_positions {
                analyzer.record_use(reg as u16, pos);
            }
        }
    }

    for (&reg, _use_positions) in &scan.uses {
        if reg >= base && !scan.defs.contains_key(&reg) {
            analyzer.record_def(reg as u16, 0);
        }
    }

    let ranges = analyzer.analyze_with_back_edges(all_regs, &back_edges);
    if ranges.is_empty() {
        return;
    }

    let raw_mapping = color_with_base(&ranges, base);

    let mapping: HashMap<u8, u8> = raw_mapping
        .into_iter()
        .filter(|&(old, new)| old != new)
        .collect();

    if mapping.is_empty() {
        return;
    }

    if !verify_call_constraints(&proto.chunk.code, &mapping) {
        return;
    }

    if !verify_build_array_constraints(&proto.chunk.code, &mapping) {
        return;
    }

    let new_max = scan
        .defs
        .keys()
        .map(|&r| mapping.get(&r).copied().unwrap_or(r))
        .chain(
            scan.uses
                .keys()
                .map(|&r| mapping.get(&r).copied().unwrap_or(r)),
        )
        .max()
        .unwrap_or(0);

    let new_register_count = new_max as u16 + 1;
    if new_register_count >= proto.register_count {
        return;
    }

    remap_bytecode(&mut proto.chunk.code, &mapping);
    proto.register_count = new_register_count;
}

fn verify_build_array_constraints(code: &[u16], mapping: &HashMap<u8, u8>) -> bool {
    let mut offset = 0;
    while offset < code.len() {
        let info = match decode(code, offset) {
            Some(i) => i,
            None => break,
        };

        if let Some(op) = OpCode::from_u16(code[offset]) {
            if matches!(op, OpCode::BuildArray) {
                let w1 = if offset + 1 < code.len() {
                    code[offset + 1]
                } else {
                    0
                };
                let w2 = if offset + 2 < code.len() {
                    code[offset + 2]
                } else {
                    0
                };
                let start = (w1 & 0xff) as u8;
                let count = (w2 >> 8) as u8;
                if count > 1 {
                    let mapped_start = mapping.get(&start).copied().unwrap_or(start);
                    for i in 1..count {
                        let orig = start.wrapping_add(i);
                        let mapped = mapping.get(&orig).copied().unwrap_or(orig);
                        if mapped != mapped_start.wrapping_add(i) {
                            return false;
                        }
                    }
                }
            }
        }

        offset += info.len;
    }
    true
}
