use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use varn_core::OpCode;
use varn_types::{chunk::PoolEntry, FunctionProto};

thread_local! {
    pub static OPTIMIZE_TIME: Cell<Duration> = Cell::new(Duration::ZERO);
    pub static OPTIMIZE_ENABLED: Cell<bool> = Cell::new(true);
}

use crate::liveness::{LiveRange, LivenessAnalyzer};

pub(crate) use varn_types::bytecode::decode;

struct ScanResult {
    defs: HashMap<u8, usize>,

    uses: HashMap<u8, Vec<usize>>,

    call_sites: Vec<(usize, u8, u8)>,
}

fn scan_bytecode(code: &[u16], constants: &[PoolEntry]) -> ScanResult {
    let mut defs: HashMap<u8, usize> = HashMap::new();
    let mut uses: HashMap<u8, Vec<usize>> = HashMap::new();
    let mut call_sites: Vec<(usize, u8, u8)> = Vec::new();
    let mut open_captures: Vec<u8> = Vec::new();

    let mut offset = 0;
    let mut instr_idx = 0usize;

    while offset < code.len() {
        let info = match decode(code, offset, constants) {
            Some(i) => i,
            None => break,
        };

        if let Some(OpCode::MakeClosure) = OpCode::from_u16(code[offset]) {
            let uv_count = (code[offset + 1] & 0xff) as usize;
            for i in 0..uv_count {
                let desc = code.get(offset + 3 + i).copied().unwrap_or(0);
                let is_local = (desc >> 8) as u8;
                if is_local == 1 {
                    open_captures.push((desc & 0xff) as u8);
                }
            }
        }

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
        if let Some((arg_start, arg_count)) = info.call_args {
            call_sites.push((instr_idx, arg_start, arg_count));
        }

        offset += info.len;
        instr_idx += 1;
    }

    let last = instr_idx.saturating_sub(1);
    for reg in open_captures {
        uses.entry(reg).or_insert_with(Vec::new).push(last);
    }

    ScanResult {
        defs,
        uses,
        call_sites,
    }
}

fn color_with_base(ranges: &[LiveRange], base: u8, copies: &[(u8, u8)]) -> HashMap<u8, u8> {
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

        let mut color_opt = None;
        for &(u, v) in copies {
            if u == reg {
                if let Some(&c) = coloring.get(&v) {
                    if !neighbor_colors.contains(&c) {
                        color_opt = Some(c);
                        break;
                    }
                }
            } else if v == reg {
                if let Some(&c) = coloring.get(&u) {
                    if !neighbor_colors.contains(&c) {
                        color_opt = Some(c);
                        break;
                    }
                }
            }
        }

        let color = color_opt.unwrap_or_else(|| {
            (base..)
                .find(|c| !neighbor_colors.contains(c))
                .unwrap_or(base)
        });
        coloring.insert(reg, color);
    }

    coloring
}

fn verify_call_constraints(
    code: &[u16],
    constants: &[PoolEntry],
    mapping: &HashMap<u8, u8>,
) -> bool {
    let mut offset = 0;
    while offset < code.len() {
        let info = match decode(code, offset, constants) {
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

fn verify_callee_frame_constraints(scan: &ScanResult, mapping: &HashMap<u8, u8>) -> bool {
    let m = |r: u8| mapping.get(&r).copied().unwrap_or(r);
    for &(call_idx, arg_start, arg_count) in &scan.call_sites {
        let mapped_start = m(arg_start);
        let arg_end = arg_start.wrapping_add(arg_count);
        for (&reg, &def_idx) in &scan.defs {
            if def_idx >= call_idx {
                continue;
            }
            if reg >= arg_start && reg < arg_end {
                continue;
            }
            let live_across = scan
                .uses
                .get(&reg)
                .is_some_and(|us| us.iter().any(|&u| u > call_idx));
            if live_across && m(reg) >= mapped_start {
                return false;
            }
        }
    }
    true
}

fn remap_bytecode(code: &mut Vec<u16>, constants: &[PoolEntry], mapping: &HashMap<u8, u8>) {
    fn m(mapping: &HashMap<u8, u8>, r: u8) -> u8 {
        mapping.get(&r).copied().unwrap_or(r)
    }

    let mut offset = 0;
    while offset < code.len() {
        let info = match decode(code, offset, constants) {
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
            let hi4 = (w4 >> 8) as u8;
            let lo4 = (w4 & 0xff) as u8;

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
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), 0);
                }

                OpCode::ArrayPush | OpCode::Inherit => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }
                OpCode::Yield | OpCode::Return => {
                    code[offset + 1] = pack(hi1, m(mapping, lo1));
                }
                OpCode::Throw => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }
                OpCode::StoreUpvalue => {
                    code[offset + 1] = pack(hi1, m(mapping, lo1));
                }
                OpCode::CloseUpvalue => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }
                OpCode::AssertNotNull => {
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }
                OpCode::ObjectMerge => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), 0);
                }

                OpCode::Jump | OpCode::Loop => {}

                OpCode::LoadConst
                | OpCode::LoadInt
                | OpCode::LoadGlobal
                | OpCode::LoadGlobalIdx => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                }

                OpCode::MakeClass => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), 0);
                }

                OpCode::GetSuper => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                }

                OpCode::StoreGlobal
                | OpCode::DefineGlobal
                | OpCode::StoreGlobalIdx
                | OpCode::DefineGlobalIdx => {
                    code[offset + 1] = pack(m(mapping, hi1), 0);
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
                | OpCode::StrSlice
                | OpCode::In
                | OpCode::Instanceof
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
                | OpCode::NeqFloat => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }
                OpCode::ArrayExtend => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), 0);
                }
                OpCode::Spawn => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }
                OpCode::InvokeRuntimeStatic => {
                    code[offset + 1] = pack(m(mapping, hi1), 0);
                    code[offset + 3] = pack(hi3, m(mapping, lo3));
                    code[offset + 4] = pack(m(mapping, hi4), lo4);
                }

                OpCode::LoadModuleSlot => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }
                OpCode::GetPropertyMaybe | OpCode::GetFixedField | OpCode::GetSymbol => {
                    let new_dest = m(mapping, dest0);
                    let new_obj = if dest0 == hi1 {
                        new_dest
                    } else {
                        m(mapping, hi1)
                    };
                    code[offset] = pack_op(op, new_dest);
                    code[offset + 1] = pack(new_obj, lo1);
                }

                OpCode::BindMethod => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }

                OpCode::MakeEnumVariant => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }

                OpCode::SetFixedField => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
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
                    let new_dest = m(mapping, dest0);
                    let new_obj = if dest0 == hi1 {
                        new_dest
                    } else {
                        m(mapping, hi1)
                    };
                    code[offset] = pack_op(op, new_dest);
                    code[offset + 1] = pack(new_obj, lo1);
                }
                OpCode::SetProperty => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }

                OpCode::Call => {
                    let arg_count = hi2;
                    let arg_start = lo2;
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 2] = pack(arg_count, m(mapping, arg_start));
                }

                OpCode::CallSelf => {
                    let arg_count = hi2;
                    let arg_start = lo2;
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                    code[offset + 2] = pack(arg_count, m(mapping, arg_start));
                }

                OpCode::CallSpread => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 2] = pack(hi2, m(mapping, lo2));
                }

                OpCode::InvokeVirtual => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 3] = pack(hi3, m(mapping, lo3));
                }

                OpCode::CallMethod => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                    code[offset + 3] = pack(hi3, m(mapping, lo3));
                }

                OpCode::LoadModule | OpCode::StoreModuleSlot => {
                    code[offset] = pack_op(op, m(mapping, dest0));
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
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, start as u8));
                }

                OpCode::GetIndex | OpCode::ArrayGetIndex => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }

                OpCode::SetIndex | OpCode::ArraySetIndex => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }

                OpCode::ObjectRest => {
                    code[offset + 1] = pack(m(mapping, hi1), m(mapping, lo1));
                }

                OpCode::Intrinsic => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                }
                OpCode::CallNativeOp => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                }
                OpCode::LoadStaticFn => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                }
                OpCode::AddImm | OpCode::SubImm => {
                    code[offset] = pack_op(op, m(mapping, dest0));
                    code[offset + 1] = pack(m(mapping, hi1), lo1);
                }

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
    if proto.is_async || proto.is_generator {
        return;
    }

    let fixed_count = proto.arity + if proto.has_this { 1 } else { 0 };
    let base = fixed_count as u8;

    if proto.chunk.code.is_empty() {
        return;
    }

    let back_edges =
        varn_types::loop_analysis::collect_back_edges(&proto.chunk.code, &proto.chunk.constants);
    let scan = scan_bytecode(&proto.chunk.code, &proto.chunk.constants);

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

    let mut copies = Vec::new();
    let mut offset = 0;
    while offset < proto.chunk.code.len() {
        if let Some(info) = decode(&proto.chunk.code, offset, &proto.chunk.constants) {
            if OpCode::from_u16(proto.chunk.code[offset]) == Some(OpCode::Move) {
                let w1 = proto.chunk.code[offset + 1];
                let dest = (proto.chunk.code[offset] >> 8) as u8;
                let src = (w1 >> 8) as u8;
                if dest >= base && src >= base {
                    copies.push((dest, src));
                }
            }
            offset += info.len;
        } else {
            break;
        }
    }

    let raw_mapping = color_with_base(&ranges, base, &copies);

    let mapping: HashMap<u8, u8> = raw_mapping
        .into_iter()
        .filter(|&(old, new)| old != new)
        .collect();

    if mapping.is_empty() {
        return;
    }

    if !verify_call_constraints(&proto.chunk.code, &proto.chunk.constants, &mapping) {
        return;
    }

    if !verify_callee_frame_constraints(&scan, &mapping) {
        return;
    }

    if !verify_build_array_constraints(&proto.chunk.code, &proto.chunk.constants, &mapping) {
        return;
    }

    if !verify_build_object_with_shape_constraints(
        &proto.chunk.code,
        &proto.chunk.constants,
        &mapping,
    ) {
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

    remap_bytecode(&mut proto.chunk.code, &proto.chunk.constants, &mapping);
    proto.register_count = new_register_count;
}

fn verify_build_array_constraints(
    code: &[u16],
    constants: &[PoolEntry],
    mapping: &HashMap<u8, u8>,
) -> bool {
    let mut offset = 0;
    while offset < code.len() {
        let info = match decode(code, offset, constants) {
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
                        let orig = start.wrapping_add(i as u8);
                        let mapped = mapping.get(&orig).copied().unwrap_or(orig);
                        if mapped != mapped_start.wrapping_add(i as u8) {
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

fn verify_build_object_with_shape_constraints(
    code: &[u16],
    constants: &[PoolEntry],
    mapping: &HashMap<u8, u8>,
) -> bool {
    let mut offset = 0;
    while offset < code.len() {
        let info = match decode(code, offset, constants) {
            Some(i) => i,
            None => break,
        };

        if let Some(op) = OpCode::from_u16(code[offset]) {
            if matches!(op, OpCode::BuildObjectWithShape) {
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
                let shape_idx = w2 as usize;
                let count = match constants.get(shape_idx) {
                    Some(PoolEntry::Shape(k)) => k.len(),
                    _ => 0,
                };
                if count > 1 {
                    let mapped_start = mapping.get(&start).copied().unwrap_or(start);
                    for i in 1..count {
                        let orig = start.wrapping_add(i as u8);
                        let mapped = mapping.get(&orig).copied().unwrap_or(orig);
                        if mapped != mapped_start.wrapping_add(i as u8) {
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
