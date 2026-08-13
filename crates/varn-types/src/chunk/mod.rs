//! Bytecode container types: the chunk itself, its constant pool, the line
//! table, per-function metadata and the inline caches hanging off it.

mod inline_cache;
mod lines;
mod literal;
mod pool;
mod proto;

pub use inline_cache::{CacheEntry, FeedbackVector, ICKind, PolyICSlot, SiteProfile, INVALID_CACHE_SHAPE};
pub use lines::{LineEntry, LineMapping};
pub use literal::Literal;
pub use pool::PoolEntry;
pub use proto::{ExceptionRange, FunctionProto};

use literal::rc_str_serde;
use std::rc::Rc;
use varn_core::OpCode;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Chunk {
    pub code: Vec<u16>,

    pub lines: LineMapping,

    pub constants: Vec<PoolEntry>,

    #[serde(skip)]
    pub constants_map: rustc_hash::FxHashMap<PoolEntry, u16>,

    #[serde(with = "rc_str_serde")]
    pub source_file: Rc<str>,

    #[serde(skip)]
    pub module_id: Option<varn_core::ModuleId>,
}

impl PartialEq for Chunk {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code && self.lines == other.lines && self.constants == other.constants
    }
}

impl Eq for Chunk {}

impl std::hash::Hash for Chunk {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.code.hash(state);
        self.lines.hash(state);
        self.constants.hash(state);
    }
}

impl Default for Chunk {
    fn default() -> Self {
        Self {
            code: Vec::new(),
            lines: LineMapping::default(),
            constants: Vec::new(),
            constants_map: rustc_hash::FxHashMap::default(),
            source_file: Rc::from(""),
            module_id: None,
        }
    }
}

impl Chunk {
    pub fn new() -> Self {
        Chunk::default()
    }

    pub fn write(&mut self, word: u16, line: u32) {
        self.code.push(word);
        self.lines.add(line);
    }

    pub fn emit(&mut self, op: OpCode, line: u32) {
        self.write(op as u8 as u16, line);
    }

    pub fn emit1(&mut self, op: OpCode, operand: u16, line: u32) {
        self.write(op as u8 as u16, line);
        self.write(operand, line);
    }

    #[inline(always)]
    pub fn pack(r1: u8, r2: u8) -> u16 {
        ((r1 as u16) << 8) | (r2 as u16)
    }

    #[inline(always)]
    pub fn pack_op(op: OpCode, reg: u8) -> u16 {
        ((reg as u16) << 8) | (op as u8 as u16)
    }

    pub fn emit_rr(&mut self, op: OpCode, dest: u8, src: u8, line: u32) {
        match op {
            OpCode::LoadNull | OpCode::LoadTrue | OpCode::LoadFalse => {
                self.write(Self::pack_op(op, dest), line);
            }
            OpCode::Move if dest == src => {}
            _ => {
                self.write(Self::pack_op(op, dest), line);
                self.write(Self::pack(src, 0), line);
            }
        }
    }

    pub fn emit_rrr(&mut self, op: OpCode, dest: u8, src1: u8, src2: u8, line: u32) {
        self.write(Self::pack_op(op, dest), line);
        self.write(Self::pack(src1, src2), line);
    }

    pub fn emit_rrc(&mut self, op: OpCode, dest: u8, src: u8, const_idx: u16, line: u32) {
        self.write(Self::pack_op(op, dest), line);
        self.write(Self::pack(src, 0), line);
        self.write(const_idx, line);
    }

    pub fn emit_rrc_ic(
        &mut self,
        op: OpCode,
        dest: u8,
        src: u8,
        const_idx: u16,
        cs_idx: u8,
        line: u32,
    ) {
        self.write(Self::pack_op(op, dest), line);
        self.write(Self::pack(src, cs_idx), line);
        self.write(const_idx, line);
    }

    pub fn emit_rc(&mut self, op: OpCode, dest: u8, const_idx: u16, line: u32) {
        self.write(Self::pack_op(op, dest), line);
        self.write(const_idx, line);
    }

    pub fn emit_jump(&mut self, op: OpCode, line: u32) -> usize {
        self.write(op as u8 as u16, line);
        let patch_pos = self.code.len();
        self.write(0xFFFF, line);
        self.write(0xFFFF, line);
        patch_pos
    }

    pub fn emit_cond_jump(&mut self, op: OpCode, cond_reg: u8, line: u32) -> usize {
        self.write(Self::pack_op(op, cond_reg), line);
        let patch_pos = self.code.len();
        self.write(0xFFFF, line);
        self.write(0xFFFF, line);
        patch_pos
    }

    pub fn emit_loop(&mut self, loop_start: usize, line: u32) {
        let offset = self.code.len() - loop_start + 3;
        let offset = u32::try_from(offset).expect("loop offset overflows u32");
        self.write(OpCode::Loop as u8 as u16, line);
        self.write((offset >> 16) as u16, line);
        self.write((offset & 0xFFFF) as u16, line);
    }

    pub fn add_constant(&mut self, entry: PoolEntry) -> u16 {
        if let Some(&idx) = self.constants_map.get(&entry) {
            return idx;
        }
        let idx = self.constants.len();
        if idx >= (u16::MAX - 1) as usize {
            return 0xFFFF;
        }
        let idx_u16 = idx as u16;
        self.constants.push(entry.clone());
        self.constants_map.insert(entry, idx_u16);
        idx_u16
    }

    pub fn add_str(&mut self, s: impl AsRef<str>) -> u16 {
        self.add_constant(PoolEntry::Literal(Literal::Str(Rc::from(s.as_ref()))))
    }

    pub fn add_shape(&mut self, keys: Vec<Rc<str>>) -> u16 {
        self.add_constant(PoolEntry::Shape(keys))
    }

    pub fn add_int(&mut self, n: i64) -> u16 {
        self.add_constant(PoolEntry::Literal(Literal::Int(n)))
    }

    pub fn add_symbol(&mut self, s: crate::value::RuntimeSymbol) -> u16 {
        self.add_constant(PoolEntry::Literal(Literal::Symbol(s)))
    }

    pub fn emit_load_int(&mut self, dest: u8, n: i64, line: u32) {
        match n {
            0 => self.write(Self::pack_op(OpCode::LoadIntZero, dest), line),
            1 => self.write(Self::pack_op(OpCode::LoadIntOne, dest), line),
            -1 => self.write(Self::pack_op(OpCode::LoadIntMinusOne, dest), line),
            _ if n >= i16::MIN as i64 && n <= i16::MAX as i64 => {
                self.write(Self::pack_op(OpCode::LoadInt, dest), line);
                self.write(n as i16 as u16, line);
            }
            _ => {
                let idx = self.add_int(n);
                self.emit_rc(OpCode::LoadConst, dest, idx, line);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.code.len()
    }

    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }
}
