use std::cell::RefCell;
use std::rc::Rc;
use varn_core::{IntrinsicType, OpCode};

/// Serde variant labels for [`Literal`], one per kind, sourced from the single
/// canonical [`IntrinsicType`] names. Round-trip keys on the numeric index, so
/// these are identifiers only — but they stay the one canonical representation.
static LITERAL_VARIANTS: [&str; 9] = [
    IntrinsicType::Null.as_str(),
    IntrinsicType::Bool.as_str(),
    IntrinsicType::Int.as_str(),
    IntrinsicType::Float.as_str(),
    IntrinsicType::Str.as_str(),
    IntrinsicType::BigInt.as_str(),
    IntrinsicType::Decimal.as_str(),
    IntrinsicType::Symbol.as_str(),
    IntrinsicType::Char.as_str(),
];

mod rc_str_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::rc::Rc;
    pub fn serialize<S: Serializer>(s: &Rc<str>, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Rc<str>, D::Error> {
        Ok(Rc::from(String::deserialize(de)?))
    }
}

mod opt_rc_str_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::rc::Rc;
    pub fn serialize<S: Serializer>(s: &Option<Rc<str>>, ser: S) -> Result<S::Ok, S::Error> {
        match s {
            Some(v) => ser.serialize_some(v.as_ref()),
            None => ser.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Rc<str>>, D::Error> {
        Ok(Option::<String>::deserialize(de)?.map(|s| Rc::from(s)))
    }
}

#[derive(Clone, Debug)]
pub enum Literal {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    BigInt(i128),
    Decimal(rust_decimal::Decimal),
    Symbol(crate::value::RuntimeSymbol),
    Char(char),
}

impl PartialEq for Literal {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => {
                let a = if *a == 0.0 { 0.0f64 } else { *a };
                let b = if *b == 0.0 { 0.0f64 } else { *b };
                a.to_bits() == b.to_bits()
            }
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::BigInt(a), Self::BigInt(b)) => a == b,
            (Self::Decimal(a), Self::Decimal(b)) => a == b,
            (Self::Symbol(a), Self::Symbol(b)) => a == b,
            (Self::Char(a), Self::Char(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Literal {}

impl serde::Serialize for Literal {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self {
            Literal::Null => ser.serialize_unit_variant("Literal", 0, LITERAL_VARIANTS[0]),
            Literal::Bool(b) => ser.serialize_newtype_variant("Literal", 1, LITERAL_VARIANTS[1], b),
            Literal::Int(i) => ser.serialize_newtype_variant("Literal", 2, LITERAL_VARIANTS[2], i),
            Literal::Float(f) => {
                ser.serialize_newtype_variant("Literal", 3, LITERAL_VARIANTS[3], f)
            }
            Literal::Str(s) => {
                ser.serialize_newtype_variant("Literal", 4, LITERAL_VARIANTS[4], s.as_ref())
            }
            Literal::BigInt(n) => {
                ser.serialize_newtype_variant("Literal", 5, LITERAL_VARIANTS[5], n)
            }
            Literal::Decimal(d) => {
                let bits = d.serialize();
                ser.serialize_newtype_variant("Literal", 6, LITERAL_VARIANTS[6], &bits)
            }
            Literal::Symbol(s) => {
                ser.serialize_newtype_variant("Literal", 7, LITERAL_VARIANTS[7], s)
            }
            Literal::Char(c) => ser.serialize_newtype_variant("Literal", 8, LITERAL_VARIANTS[8], c),
        }
    }
}

impl<'de> serde::Deserialize<'de> for Literal {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de::{self, EnumAccess, VariantAccess, Visitor};
        use std::fmt;

        struct LiteralVisitor;

        impl<'de> Visitor<'de> for LiteralVisitor {
            type Value = Literal;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("Literal enum")
            }
            fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
                let (idx, variant): (u32, _) = data.variant()?;
                match idx {
                    0 => {
                        variant.unit_variant()?;
                        Ok(Literal::Null)
                    }
                    1 => Ok(Literal::Bool(variant.newtype_variant()?)),
                    2 => Ok(Literal::Int(variant.newtype_variant()?)),
                    3 => Ok(Literal::Float(variant.newtype_variant()?)),
                    4 => Ok(Literal::Str(Rc::from(variant.newtype_variant::<String>()?))),
                    5 => Ok(Literal::BigInt(variant.newtype_variant()?)),
                    6 => {
                        let bits: [u8; 16] = variant.newtype_variant()?;
                        Ok(Literal::Decimal(rust_decimal::Decimal::deserialize(bits)))
                    }
                    7 => Ok(Literal::Symbol(variant.newtype_variant()?)),
                    8 => Ok(Literal::Char(variant.newtype_variant()?)),
                    _ => Err(de::Error::unknown_variant(
                        &idx.to_string(),
                        &["0", "1", "2", "3", "4", "5", "6", "7", "8"],
                    )),
                }
            }
        }

        de.deserialize_enum("Literal", &LITERAL_VARIANTS, LiteralVisitor)
    }
}

impl std::hash::Hash for Literal {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Null => {}
            Self::Bool(b) => b.hash(state),
            Self::Int(i) => i.hash(state),
            Self::Float(f) => {
                let f = if *f == 0.0 { 0.0f64 } else { *f };
                f.to_bits().hash(state);
            }
            Self::Str(s) => s.hash(state),
            Self::BigInt(b) => b.hash(state),
            Self::Decimal(d) => d.hash(state),
            Self::Symbol(s) => s.hash(state),
            Self::Char(c) => c.hash(state),
        }
    }
}

#[derive(Clone)]
pub enum PoolEntry {
    Literal(Literal),
    Function(std::rc::Rc<FunctionProto>),
    Shape(Vec<std::rc::Rc<str>>),
}

impl std::fmt::Debug for PoolEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Literal(lit) => f.debug_tuple("Literal").field(lit).finish(),
            Self::Function(proto) => f
                .debug_struct("Function")
                .field("name", &proto.name)
                .field("arity", &proto.arity)
                .finish_non_exhaustive(),
            Self::Shape(keys) => f.debug_tuple("Shape").field(keys).finish(),
        }
    }
}

impl std::hash::Hash for PoolEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Literal(lit) => lit.hash(state),
            Self::Function(f) => {
                std::rc::Rc::as_ptr(f).hash(state);
            }
            Self::Shape(keys) => keys.hash(state),
        }
    }
}

impl PartialEq for PoolEntry {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Literal(l1), Self::Literal(l2)) => l1 == l2,
            (Self::Shape(k1), Self::Shape(k2)) => k1 == k2,
            (Self::Function(f1), Self::Function(f2)) => std::rc::Rc::ptr_eq(f1, f2),
            _ => false,
        }
    }
}

impl Eq for PoolEntry {}

impl serde::Serialize for PoolEntry {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self {
            PoolEntry::Literal(l) => ser.serialize_newtype_variant("PoolEntry", 0, "Literal", l),
            PoolEntry::Function(f) => {
                ser.serialize_newtype_variant("PoolEntry", 1, "Function", f.as_ref())
            }
            PoolEntry::Shape(keys) => {
                let strs: Vec<&str> = keys.iter().map(|s| s.as_ref()).collect();
                ser.serialize_newtype_variant("PoolEntry", 2, "Shape", &strs)
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for PoolEntry {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de::{self, EnumAccess, VariantAccess, Visitor};
        use std::fmt;

        struct PoolEntryVisitor;

        impl<'de> Visitor<'de> for PoolEntryVisitor {
            type Value = PoolEntry;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("PoolEntry enum")
            }
            fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
                let (idx, variant): (u32, _) = data.variant()?;
                match idx {
                    0 => Ok(PoolEntry::Literal(variant.newtype_variant()?)),
                    1 => Ok(PoolEntry::Function(Rc::new(
                        variant.newtype_variant::<FunctionProto>()?,
                    ))),
                    2 => {
                        let strs = variant.newtype_variant::<Vec<String>>()?;
                        Ok(PoolEntry::Shape(
                            strs.into_iter().map(|s| Rc::from(s)).collect(),
                        ))
                    }
                    _ => Err(de::Error::unknown_variant(
                        &idx.to_string(),
                        &["0", "1", "2"],
                    )),
                }
            }
        }

        de.deserialize_enum(
            "PoolEntry",
            &["Literal", "Function", "Shape"],
            PoolEntryVisitor,
        )
    }
}

impl PoolEntry {
    pub fn as_str(&self) -> Option<&str> {
        if let PoolEntry::Literal(Literal::Str(s)) = self {
            Some(s.as_ref())
        } else {
            None
        }
    }
}

pub const INVALID_CACHE_SHAPE: u32 = 0;

#[derive(Clone, Copy, Default, Debug, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct CacheEntry {
    pub id: u32,
    pub slot: u16,
    pub is_class: u8,
    pub vtable_ver: u8,
}

impl CacheEntry {
    #[inline(always)]
    pub fn to_u64(self) -> u64 {
        unsafe { std::mem::transmute(self) }
    }

    #[inline(always)]
    pub fn from_u64(val: u64) -> Self {
        unsafe { std::mem::transmute(val) }
    }

    #[inline(always)]
    pub fn matches(&self, other: &CacheEntry) -> bool {
        self.id == other.id && self.is_class == other.is_class
    }
}

#[derive(Clone, Debug)]
pub struct PolyICSlot {
    pub entries: [CacheEntry; 8],

    pub next: u8,

    last_hit: u8,
}

impl PolyICSlot {
    pub fn new() -> Self {
        Self {
            entries: [CacheEntry::default(); 8],
            next: 0,
            last_hit: 0,
        }
    }

    pub fn find_or_insert(&mut self, entry: CacheEntry) {
        for (i, e) in self.entries.iter_mut().enumerate() {
            if e.matches(&entry) {
                *e = entry;
                self.last_hit = i as u8;

                self.next = (self.last_hit + 4) & 0x7;
                return;
            }
        }

        self.entries[self.next as usize] = entry;
        self.next = (self.next + 1) & 0x7;
    }
}

#[derive(Clone, Debug, Default)]
pub struct SiteProfile {
    pub ids: [u32; 4],
    pub count: u32,
    pub megamorphic: bool,
}

impl SiteProfile {
    #[inline(always)]
    pub fn observe(&mut self, id: u32) {
        if self.megamorphic || id == 0 {
            return;
        }
        self.count = self.count.saturating_add(1);
        for slot in &mut self.ids {
            if *slot == id {
                return;
            }
            if *slot == 0 {
                *slot = id;
                return;
            }
        }
        self.megamorphic = true;
    }

    pub fn is_monomorphic(&self) -> bool {
        !self.megamorphic && self.ids[1] == 0 && self.ids[0] != 0
    }

    pub fn is_polymorphic(&self) -> bool {
        !self.megamorphic && self.ids[1] != 0
    }
}

#[derive(Clone, Debug, Default)]
pub struct FeedbackVector {
    pub sites: Vec<SiteProfile>,
}

impl FeedbackVector {
    pub fn new(site_count: usize) -> Self {
        Self {
            sites: vec![SiteProfile::default(); site_count],
        }
    }

    #[inline(always)]
    pub fn observe(&mut self, site_idx: usize, id: u32) {
        if let Some(site) = self.sites.get_mut(site_idx) {
            site.observe(id);
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash)]
pub struct LineEntry {
    pub count: u32,
    pub line: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash, Default)]
pub struct LineMapping {
    pub entries: Vec<LineEntry>,

    #[serde(skip)]
    starts: Vec<u32>,
}

impl LineMapping {
    pub fn add(&mut self, line: u32) {
        if let Some(last) = self.entries.last_mut() {
            if last.line == line {
                last.count += 1;

                return;
            }
        }

        let next_start: u32 = self.starts.last().copied().unwrap_or(0)
            + self.entries.last().map(|e| e.count).unwrap_or(0);
        self.starts.push(next_start);
        self.entries.push(LineEntry { count: 1, line });
    }

    pub fn get_line(&self, instruction_idx: usize) -> u32 {
        if self.starts.len() != self.entries.len() {
            let mut base = 0usize;
            for entry in &self.entries {
                let next = base + entry.count as usize;
                if instruction_idx < next {
                    return entry.line;
                }
                base = next;
            }
            return 0;
        }
        let idx = instruction_idx as u32;

        let pos = self.starts.partition_point(|&s| s <= idx);
        if pos == 0 {
            return 0;
        }
        self.entries[pos - 1].line
    }

    pub fn truncate(&mut self, instruction_idx: usize) {
        let mut current = 0;
        let mut to_remove_from = None;
        for (i, entry) in self.entries.iter_mut().enumerate() {
            let next = current + entry.count as usize;
            if instruction_idx < next {
                let keep = instruction_idx - current;
                if keep == 0 {
                    to_remove_from = Some(i);
                } else {
                    entry.count = keep as u32;
                    to_remove_from = Some(i + 1);
                }
                break;
            }
            current = next;
        }
        if let Some(idx) = to_remove_from {
            self.entries.truncate(idx);
            self.starts.truncate(idx);
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ExceptionRange {
    pub try_start_ip: u32,
    pub try_end_ip: u32,
    pub catch_ip: u32,
    pub err_reg: u8,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FunctionProto {
    #[serde(with = "opt_rc_str_serde")]
    pub name: Option<Rc<str>>,
    pub arity: usize,

    pub export_names: Vec<Rc<str>>,

    pub register_count: u16,
    pub has_rest: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub has_this: bool,
    pub upvalue_count: usize,
    pub cache_count: usize,
    pub chunk: Chunk,

    #[serde(default)]
    pub required_caps: Vec<std::rc::Rc<str>>,

    #[serde(default)]
    pub register_meta: Vec<crate::register_meta::RegisterMeta>,

    #[serde(default)]
    pub exception_table: Vec<ExceptionRange>,

    /// Declared parameter slot kinds, in parameter order (from the checker
    /// via HirParam). The Cranelift router requires all-Int parameters
    /// before it may emit unboxed entry code.
    #[serde(default)]
    pub param_kinds: Vec<crate::register_meta::SlotKind>,

    /// Declared return slot kind. `Dynamic` when unannotated — the
    /// Cranelift wrapper may only re-tag when this proves Int.
    #[serde(default = "slot_kind_dynamic")]
    pub return_kind: crate::register_meta::SlotKind,

    /// Runtime cache: `PoolEntry::Shape` constants resolved to their
    /// `Shape` in the (globally cached) transition tree, so object literals
    /// don't re-derive the shape key-by-key on every allocation. Protos hold
    /// at most a handful of shape constants, so a linear scan beats hashing.
    #[serde(skip, default)]
    pub resolved_shapes: RefCell<Vec<(u32, Rc<crate::Shape>)>>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_entry: std::cell::Cell<Option<usize>>,

    /// Address of this proto's Cranelift RAW entry — the unboxed
    /// `fn(exec_ctx, args…) -> i64` body, callable clif→clif without going
    /// back through the VM frame loop. `0` means "no direct entry": either
    /// the proto is not compiled yet, its compilation failed, or it took the
    /// frame-aware lowering (whose raw needs a callee frame the caller cannot
    /// supply).
    ///
    /// Call sites embed the ADDRESS OF THIS CELL and load it at run time
    /// rather than baking the entry in. Callers compile before their callees
    /// — a caller reaches its tier threshold first, by definition — so a
    /// compile-time snapshot would be `None` for essentially every call and
    /// would never be revisited. The extra load is what makes the direct call
    /// reachable at all.
    #[serde(skip)]
    #[serde(default)]
    pub clif_raw: std::cell::Cell<usize>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_code: std::cell::RefCell<Option<Rc<dyn std::any::Any>>>,

    #[serde(skip)]
    #[serde(default)]
    pub jit_failed: std::cell::Cell<bool>,

    /// Which `ExecCtx` the code in `jit_entry`/`clif_raw` was compiled for.
    ///
    /// Compiled code is NOT context-independent: `LoadConst` bakes the
    /// constant's `VmValue` — a handle into one heap — as an immediate, and the
    /// linker bakes addresses of that context's globals and sibling protos.
    /// A proto, by contrast, outlives any single context: it is owned by the
    /// module chunk and survives every re-execution of the program. Running
    /// yesterday's code against today's heap reads whatever object now sits at
    /// the baked index — `"a" + <object> + "b"` where a literal belonged.
    /// A frame entry may only use the entry when this matches the running
    /// context's epoch; anything else recompiles.
    #[serde(skip)]
    #[serde(default)]
    pub jit_epoch: std::cell::Cell<u64>,

    /// When that code was built, on the VM's monotonic compile clock. A heap
    /// copied off another inherits the entries its ancestor had already built
    /// at the moment of the copy, and only those — this is what orders the two.
    #[serde(skip)]
    #[serde(default)]
    pub jit_serial: std::cell::Cell<u64>,

    /// Memoised "does this function contain a back edge": `0` not looked at,
    /// `1` yes, `2` no. Decides how the tier threshold applies — see
    /// [`Self::has_backedge`].
    #[serde(skip)]
    #[serde(default)]
    pub backedge_memo: std::cell::Cell<u8>,

    #[serde(skip, default = "proto_ic_default")]
    pub ic_cache: Rc<RefCell<Vec<PolyICSlot>>>,

    #[serde(skip, default = "proto_feedback_default")]
    pub feedback: Rc<RefCell<FeedbackVector>>,

    #[serde(skip)]
    #[serde(default)]
    pub static_closure_val: std::cell::Cell<u64>,

    /// Frame entries seen so far, counted only while this proto is still
    /// uncompiled. Cranelift lowering costs ~640 µs per function against the
    /// ~17 µs the template JIT used to charge, so compiling at closure
    /// construction — as the template tier could afford — now dominates any
    /// workload that builds more functions than it runs (isolates: 144
    /// functions compiled to execute 38 JIT frames, 4.6 ms of interpretation
    /// turned into 60 ms). Compilation therefore waits for evidence the
    /// function is worth it.
    #[serde(skip)]
    #[serde(default)]
    pub jit_entry_count: std::cell::Cell<u32>,

    /// Back edges taken in this proto, across all frames. Drives the OSR
    /// trigger; unlike [`Self::jit_entry_count`] it keeps rising inside one
    /// long frame, which is the whole point — a function entered once and then
    /// looping reaches no entry threshold at all.
    #[serde(skip)]
    #[serde(default)]
    pub backedge_count: std::cell::Cell<u32>,

    /// Compiled ON-STACK REPLACEMENT entry: the same body lowered with a
    /// parameterless prologue that reloads every register from this frame's
    /// `ctx.stack` home slots and jumps straight to the block for
    /// [`Self::jit_osr_ip`].
    ///
    /// Valid ONLY for that ip, and only in the epoch recorded by
    /// [`Self::jit_osr_epoch`].
    #[serde(skip)]
    #[serde(default)]
    pub jit_osr_entry: std::cell::Cell<Option<usize>>,

    /// Which context [`Self::jit_osr_entry`] was baked for. Deliberately NOT
    /// [`Self::jit_epoch`]: that cell is re-stamped by
    /// `clif_link::adopt_if_inherited` when a copied heap adopts the NORMAL
    /// entry, and the adoption argument is per-entry — it tests
    /// [`Self::jit_serial`] against the copy's cutoff, which says nothing
    /// about an OSR variant compiled afterwards. Sharing the cell would let a
    /// copied heap enter code baked against its ancestor's objects. OSR never
    /// adopts; a mismatch just recompiles.
    #[serde(skip)]
    #[serde(default)]
    pub jit_osr_epoch: std::cell::Cell<u64>,

    /// The loop-header ip [`Self::jit_osr_entry`] resumes at. One OSR variant
    /// per proto: the first loop to prove hot wins, and a request for any other
    /// ip is refused rather than recompiling.
    #[serde(skip)]
    #[serde(default)]
    pub jit_osr_ip: std::cell::Cell<usize>,

    /// The OSR buffer, kept alive exactly like [`Self::jit_code`]: it is a
    /// separate `JitBuffer` from the normal entry's, and dropping it would
    /// unmap code the frame loop is about to jump into.
    #[serde(skip)]
    #[serde(default)]
    pub jit_osr_code: std::cell::RefCell<Option<Rc<dyn std::any::Any>>>,

    /// OSR was attempted and refused (ineligible shape, or Cranelift bailed).
    /// Latches so a frame that keeps looping does not re-attempt the same
    /// compilation every `JIT_OSR_BACKEDGES` back edges.
    #[serde(skip)]
    #[serde(default)]
    pub jit_osr_failed: std::cell::Cell<bool>,
}

fn slot_kind_dynamic() -> crate::register_meta::SlotKind {
    crate::register_meta::SlotKind::Dynamic
}

fn proto_ic_default() -> Rc<RefCell<Vec<PolyICSlot>>> {
    Rc::new(RefCell::new(Vec::new()))
}

fn proto_feedback_default() -> Rc<RefCell<FeedbackVector>> {
    Rc::new(RefCell::new(FeedbackVector::default()))
}

impl FunctionProto {
    /// Whether the body contains a back edge (`OpCode::Loop`).
    ///
    /// Tiering counts FRAME ENTRIES, which says nothing about a function that
    /// is entered once and then spins a million iterations: it never reaches
    /// any threshold, and without on-stack replacement there is no second
    /// chance to compile it. So a looping function is compiled on its first
    /// entry and only straight-line code is made to prove itself by being
    /// called again — for that shape the entry count is exactly the right
    /// evidence. Measured: a flat threshold of 8 is 5.6x on the test suite and
    /// 2.4x WORSE on `bench_matrix`; splitting the two recovers both.
    ///
    /// Walked through the shared decoder so operand words can never be
    /// mistaken for an opcode, and memoised — the answer is a property of the
    /// bytecode, which never changes.
    pub fn has_backedge(&self) -> bool {
        match self.backedge_memo.get() {
            1 => return true,
            2 => return false,
            _ => {}
        }
        let code = &self.chunk.code;
        let mut ip = 0;
        let mut found = false;
        while ip < code.len() {
            let Some(info) = crate::bytecode::decode(code, ip, &self.chunk.constants) else {
                // Undecodable: assume the worst and compile eagerly, which is
                // the pre-existing behaviour.
                found = true;
                break;
            };
            if OpCode::from_u16(code[ip]) == Some(OpCode::Loop) {
                found = true;
                break;
            }
            ip += info.len.max(1);
        }
        self.backedge_memo.set(if found { 1 } else { 2 });
        found
    }

    pub fn ensure_ic(&self) {
        let n = self.cache_count;
        if n == 0 {
            return;
        }
        let mut ic = self.ic_cache.borrow_mut();
        if ic.is_empty() {
            ic.resize_with(n, PolyICSlot::new);
        }
        let mut fb = self.feedback.borrow_mut();
        if fb.sites.is_empty() {
            *fb = FeedbackVector::new(n);
        }
    }

    /// Resolve the `PoolEntry::Shape` at `idx` to its runtime `Shape`. The
    /// first call derives it through the transition tree (which caches each
    /// step globally); later calls hit the per-proto cache.
    pub fn resolved_shape(&self, idx: usize) -> Option<Rc<crate::Shape>> {
        if let Some((_, s)) = self
            .resolved_shapes
            .borrow()
            .iter()
            .find(|(i, _)| *i as usize == idx)
        {
            return Some(Rc::clone(s));
        }
        let keys = match self.chunk.constants.get(idx) {
            Some(PoolEntry::Shape(k)) => k,
            _ => return None,
        };
        let mut shape = crate::root_shape();
        for k in keys {
            shape = shape.transition(Rc::clone(k));
        }
        self.resolved_shapes
            .borrow_mut()
            .push((idx as u32, Rc::clone(&shape)));
        Some(shape)
    }
}

impl PartialEq for FunctionProto {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.arity == other.arity
            && self.export_names == other.export_names
            && self.register_count == other.register_count
            && self.has_rest == other.has_rest
            && self.is_async == other.is_async
            && self.is_generator == other.is_generator
            && self.has_this == other.has_this
            && self.upvalue_count == other.upvalue_count
            && self.cache_count == other.cache_count
            && self.chunk == other.chunk
            && self.required_caps == other.required_caps
    }
}

impl Eq for FunctionProto {}

impl std::hash::Hash for FunctionProto {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.arity.hash(state);
        self.export_names.hash(state);
        self.register_count.hash(state);
        self.has_rest.hash(state);
        self.is_async.hash(state);
        self.is_generator.hash(state);
        self.has_this.hash(state);
        self.upvalue_count.hash(state);
        self.cache_count.hash(state);
        self.chunk.hash(state);
        self.required_caps.hash(state);
    }
}

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

    pub fn emit2(&mut self, op: OpCode, op1: u16, op2: u16, line: u32) {
        self.write(op as u8 as u16, line);
        self.write(op1, line);
        self.write(op2, line);
    }

    pub fn emit3(&mut self, op: OpCode, op1: u16, op2: u16, op3: u16, line: u32) {
        self.write(op as u8 as u16, line);
        self.write(op1, line);
        self.write(op2, line);
        self.write(op3, line);
    }

    pub fn emit4(&mut self, op: OpCode, op1: u16, op2: u16, op3: u16, op4: u16, line: u32) {
        self.write(op as u8 as u16, line);
        self.write(op1, line);
        self.write(op2, line);
        self.write(op3, line);
        self.write(op4, line);
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

    pub fn patch_jump(&mut self, patch_pos: usize) {
        let offset = self.code.len() - patch_pos - 2;
        let offset = u32::try_from(offset).expect("jump offset overflows u32");
        self.code[patch_pos] = (offset >> 16) as u16;
        self.code[patch_pos + 1] = (offset & 0xFFFF) as u16;
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
