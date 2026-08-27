use std::collections::HashMap;
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::PoolEntry;

pub(crate) struct ScanResult {
    pub(crate) defs: HashMap<u8, usize>,
    pub(crate) uses: HashMap<u8, Vec<usize>>,
    pub(crate) call_sites: Vec<(usize, u8, u8)>,
}

pub(crate) fn scan_bytecode(code: &[u16], constants: &[PoolEntry]) -> ScanResult {
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

pub(crate) fn collect_consecutive_blocks(code: &[u16], constants: &[PoolEntry]) -> Vec<(u8, u8)> {
    let mut blocks = Vec::new();
    let mut offset = 0;
    while offset < code.len() {
        let info = match decode(code, offset, constants) {
            Some(i) => i,
            None => break,
        };
        if let Some((arg_start, arg_count)) = info.call_args {
            if arg_count > 1 {
                blocks.push((arg_start, arg_count));
            }
        }
        if let Some(op) = OpCode::from_u16(code[offset]) {
            match op {
                OpCode::BuildArray | OpCode::BuildTuple => {
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
                        blocks.push((start, count));
                    }
                }
                OpCode::BuildObjectWithShape | OpCode::BuildRecord => {
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
                        blocks.push((start, count as u8));
                    }
                }
                _ => {}
            }
        }
        offset += info.len;
    }
    blocks
}
