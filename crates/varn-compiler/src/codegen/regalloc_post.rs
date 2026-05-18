use std::collections::HashMap;
use varn_core::OpCode;
use varn_types::FunctionProto;

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
    let w1 = get(1);
    let w2 = get(2);
    let w3 = get(3);
    let w4 = get(4);

    let hi1 = (w1 >> 8) as u8;
    let lo1 = (w1 & 0xff) as u8;
    let hi2 = (w2 >> 8) as u8;
    let _lo2 = (w2 & 0xff) as u8;
    let _hi3 = (w3 >> 8) as u8;
    let _lo3 = (w3 & 0xff) as u8;
    let _hi4 = (w4 >> 8) as u8;

    let s = InstrInfo::simple;
    let info = match op {
        OpCode::PopTry | OpCode::Nop => s(1, None, vec![]),

        OpCode::LoadNull | OpCode::LoadTrue | OpCode::LoadFalse => s(2, Some(hi1), vec![]),
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
        | OpCode::ObjectKeys => s(2, Some(hi1), vec![lo1]),

        OpCode::ArrayPush | OpCode::Inherit => s(2, None, vec![hi1, lo1]),
        OpCode::Yield | OpCode::Return | OpCode::Throw => s(2, None, vec![lo1]),
        OpCode::MergeExports => s(2, None, vec![hi1]),
        OpCode::StoreUpvalue => s(2, None, vec![lo1]),
        OpCode::CloseUpvalue => s(2, None, vec![lo1]),

        OpCode::Import => s(3, Some(hi1), vec![]),

        OpCode::Reexport => s(3, None, vec![]),

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

        OpCode::GetIndex => s(3, Some(hi1), vec![lo1, hi2]),

        OpCode::SetIndex => s(3, None, vec![hi1, lo1, hi2]),

        OpCode::ObjectRest => {
            let skip_count = hi2 as usize;
            s(3 + skip_count, Some(hi1), vec![lo1])
        }

        OpCode::InvokeRuntimeStatic => InstrInfo::opaque(5),

        OpCode::GetProperty | OpCode::SetProperty => InstrInfo::opaque(3),

        OpCode::CallMethod => InstrInfo::opaque(4),

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
            let w1 = get(1);
            let w2 = get(2);
            let w3 = get(3);
            let w4 = get(4);

            let hi1 = (w1 >> 8) as u8;
            let lo1 = (w1 & 0xff) as u8;
            let hi2 = (w2 >> 8) as u8;
            let lo2 = (w2 & 0xff) as u8;
            let _hi3 = (w3 >> 8) as u8;
            let _lo3 = (w3 & 0xff) as u8;
            let _hi4 = (w4 >> 8) as u8;

            #[inline(always)]
            fn pack(a: u8, b: u8) -> u16 {
                ((a as u16) << 8) | (b as u16)
            }

            if OpCode::from_u16(code[offset]) == Some(OpCode::Try) {
                let w1 = code.get(offset + 1).copied().unwrap_or(0);
                let old_err_reg = (w1 >> 8) as u8;
                let new_err_reg = m(mapping, old_err_reg);
                code[offset + 1] = ((new_err_reg as u16) << 8) | 0;
            }

            match op {
                OpCode::PopTry | OpCode::Nop => {}

                OpCode::LoadNull | OpCode::LoadTrue | OpCode::LoadFalse => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
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
                | OpCode::ObjectKeys
                | OpCode::Spawn => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }

                OpCode::ArrayPush | OpCode::Inherit => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }
                OpCode::Yield | OpCode::Return | OpCode::Throw => {
                    code[offset + 1] = pack(hi1, m(mapping, lo1));
                }
                OpCode::MergeExports => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
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

                OpCode::LoadConst
                | OpCode::LoadInt
                | OpCode::LoadGlobal
                | OpCode::LoadGlobalIdx => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }
                OpCode::MakeClass | OpCode::GetSuper => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }

                OpCode::StoreGlobal
                | OpCode::DefineGlobal
                | OpCode::StoreGlobalIdx
                | OpCode::DefineGlobalIdx => {
                    code[offset + 1] = pack(hi1, m(mapping, lo1));
                }

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
                | OpCode::ArrayExtend => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 2] = pack(m(mapping, hi2), lo2);
                }

                OpCode::StrSlice => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 2] = pack(m(mapping, hi2), m(mapping, lo2));
                }
                OpCode::In | OpCode::Instanceof => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 2] = pack(m(mapping, hi2), lo2);
                }

                OpCode::GetPropertyMaybe
                | OpCode::GetFixedField
                | OpCode::GetSymbol
                | OpCode::BindMethod => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }
                OpCode::MakeEnumVariant => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }
                OpCode::SetFixedField => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
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
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }

                OpCode::GetProperty => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }

                OpCode::SetProperty => {
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
                    code[offset + 3] = pack(_hi3, m(mapping, _lo3));
                }

                OpCode::Import => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
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

                OpCode::GetIndex => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 2] = pack(m(mapping, hi2), lo2);
                }
                OpCode::SetIndex => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 2] = pack(m(mapping, hi2), lo2);
                }

                OpCode::ObjectRest => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }

                _ => {}
            }
        }

        offset += info.len;
    }
}

fn code_contains_loop(code: &[u16]) -> bool {
    let mut offset = 0;
    while offset < code.len() {
        if let Some(op) = OpCode::from_u16(code[offset]) {
            if matches!(op, OpCode::Loop) {
                return true;
            }
        }
        match decode(code, offset) {
            Some(info) => offset += info.len,
            None => break,
        }
    }
    false
}

pub fn optimize_function(proto: &mut FunctionProto) {
    let fixed_count = proto.arity + if proto.has_this { 1 } else { 0 };
    let base = fixed_count as u8;

    if proto.chunk.code.is_empty() {
        return;
    }

    if code_contains_loop(&proto.chunk.code) {
        return;
    }

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

    let ranges = analyzer.analyze(all_regs);
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
