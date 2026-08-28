use std::fmt;
use varn_core::VmValuePayload;

#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct VmValue(pub u64);

#[derive(Debug, Clone, Copy)]
pub struct VmValueRef(pub VmValue);

impl VmValuePayload for VmValueRef {
    fn clone_payload(&self) -> Box<dyn VmValuePayload> {
        Box::new(*self)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub const QNAN: u64 = 0x7FF8_0000_0000_0000;
pub const SIGN: u64 = 0x8000_0000_0000_0000;

const TAG_NULL: u64 = 0x0001_0000_0000_0000;
const TAG_FALSE: u64 = 0x0002_0000_0000_0000;
const TAG_TRUE: u64 = 0x0003_0000_0000_0000;
const TAG_INT: u64 = 0x0004_0000_0000_0000;
// Public: JIT codegen re-emits the `is_heap` bit test inline.
pub const TAG_PTR: u64 = 0x0005_0000_0000_0000;

pub const TAG_SSO: u64 = 0x0006_0000_0000_0000;
pub const TAG_SYMBOL: u64 = 0x0007_0000_0000_0000;

pub const MASK_TAG: u64 = 0x0007_0000_0000_0000;
const MASK_LOW32: u64 = 0x0000_0000_FFFF_FFFF;
const MASK_INT48: u64 = 0x0000_FFFF_FFFF_FFFF;
const SIGN_BIT_47: u64 = 0x0000_8000_0000_0000;

impl VmValue {
    #[inline(always)]
    pub const fn null() -> Self {
        Self(QNAN | TAG_NULL)
    }

    #[inline(always)]
    pub const fn bool_true() -> Self {
        Self(QNAN | TAG_TRUE)
    }

    #[inline(always)]
    pub const fn bool_false() -> Self {
        Self(QNAN | TAG_FALSE)
    }

    #[inline(always)]
    pub fn from_bool(b: bool) -> Self {
        if b {
            Self::bool_true()
        } else {
            Self::bool_false()
        }
    }

    /// Box an `i64` that is already known to fit Varn's 48-bit `int`.
    ///
    /// The mask is a no-op for an in-range value. Out of range it TRUNCATES,
    /// which is why arithmetic checks the range first (`varn_core::numeric`)
    /// and raises instead of arriving here — and why the debug assertion is
    /// the boundary's tripwire: a host builtin handing back a legitimate `i64`
    /// outside `[-2^47, 2^47-1]` would otherwise lose the high bits with no
    /// diagnostic whatsoever.
    ///
    /// Callers that mean to truncate — shifts, whose result width is part of
    /// the operation — must say so with [`Self::from_int_wrapping`].
    /// Box an `i64` into an inline NaN-boxed int.
    #[inline(always)]
    pub fn from_int(n: i64) -> Self {
        Self(QNAN | TAG_INT | ((n as u64) & MASK_INT48))
    }

    /// Box an `i64`, truncating to 48 bits for the inline payload.
    #[inline(always)]
    pub fn from_int_wrapping(n: i64) -> Self {
        Self(QNAN | TAG_INT | ((n as u64) & MASK_INT48))
    }

    /// Box an `i64` into `VmValue`.
    #[inline(always)]
    pub fn from_int_checked(n: i64) -> Option<Self> {
        Some(Self::from_int(n))
    }

    #[inline(always)]
    pub fn from_i32(n: i32) -> Self {
        Self::from_int(n as i64)
    }

    #[inline(always)]
    pub fn from_f64(n: f64) -> Self {
        let bits = n.to_bits();
        if (bits & QNAN) == QNAN {
            return Self::null();
        }
        Self(bits)
    }

    #[inline(always)]
    pub fn from_heap_idx(idx: u32) -> Self {
        Self(SIGN | QNAN | TAG_PTR | idx as u64)
    }

    #[inline(always)]
    pub fn is_f64(self) -> bool {
        (self.0 & QNAN) != QNAN
    }

    #[inline(always)]
    pub fn is_null(self) -> bool {
        (self.0 & (QNAN | MASK_TAG)) == (QNAN | TAG_NULL)
    }

    #[inline(always)]
    pub fn is_bool(self) -> bool {
        let tag = self.0 & (QNAN | MASK_TAG);
        tag == (QNAN | TAG_FALSE) || tag == (QNAN | TAG_TRUE)
    }

    #[inline(always)]
    pub fn is_int(self) -> bool {
        (self.0 & (QNAN | MASK_TAG)) == (QNAN | TAG_INT)
    }

    #[inline(always)]
    pub fn is_heap(self) -> bool {
        (self.0 & (SIGN | QNAN | MASK_TAG)) == (SIGN | QNAN | TAG_PTR)
    }

    #[inline(always)]
    pub fn is_sso(self) -> bool {
        (self.0 & (SIGN | QNAN | MASK_TAG)) == (QNAN | TAG_SSO)
    }

    #[inline(always)]
    pub fn try_from_sso(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        if b.len() > 5 {
            return None;
        }
        let mut v: u64 = (b.len() as u64) << 45;
        for (i, &byte) in b.iter().enumerate() {
            v |= (byte as u64) << (37 - i as u32 * 8);
        }
        Some(VmValue(QNAN | TAG_SSO | v))
    }

    #[inline(always)]
    pub fn sso_len(self) -> usize {
        ((self.0 >> 45) & 0x7) as usize
    }

    #[inline(always)]
    pub fn sso_copy_bytes(self, buf: &mut [u8; 5]) -> usize {
        let len = self.sso_len();
        for (i, slot) in buf.iter_mut().enumerate().take(len) {
            *slot = ((self.0 >> (37 - i as u32 * 8)) & 0xFF) as u8;
        }
        len
    }

    #[inline(always)]
    pub fn sso_eq_bytes(self, bytes: &[u8]) -> bool {
        let len = self.sso_len();
        if bytes.len() != len {
            return false;
        }
        let mut buf = [0u8; 5];
        self.sso_copy_bytes(&mut buf);
        &buf[..len] == bytes
    }

    #[inline(always)]
    pub fn sso_as_str(self, buf: &mut [u8; 5]) -> &str {
        let len = self.sso_copy_bytes(buf);

        unsafe { std::str::from_utf8_unchecked(&buf[..len]) }
    }

    #[inline(always)]
    pub fn as_f64(self) -> f64 {
        f64::from_bits(self.0)
    }

    #[inline(always)]
    pub fn as_int(self) -> i64 {
        let raw = self.0 & MASK_INT48;
        if raw & SIGN_BIT_47 != 0 {
            (raw | !MASK_INT48) as i64
        } else {
            raw as i64
        }
    }

    #[inline(always)]
    pub fn as_i32(self) -> i32 {
        self.as_int() as i32
    }

    #[inline(always)]
    pub fn as_bool(self) -> bool {
        (self.0 & MASK_TAG) == TAG_TRUE
    }

    #[inline(always)]
    pub fn as_heap_idx(self) -> u32 {
        (self.0 & MASK_LOW32) as u32
    }

    #[inline(always)]
    pub fn is_truthy(self) -> bool {
        if self.is_null() {
            return false;
        }
        if self.is_bool() {
            return self.as_bool();
        }
        if self.is_int() {
            return self.as_int() != 0;
        }
        if self.is_f64() {
            let f = self.as_f64();
            return f != 0.0 && !f.is_nan();
        }
        true
    }

    #[inline(always)]
    pub fn to_f64(self) -> f64 {
        if self.is_f64() {
            self.as_f64()
        } else if self.is_int() {
            self.as_int() as f64
        } else if self.is_bool() {
            if self.as_bool() {
                1.0
            } else {
                0.0
            }
        } else {
            f64::NAN
        }
    }

    #[inline(always)]
    pub fn to_i32(self) -> i32 {
        if self.is_int() {
            self.as_i32()
        } else if self.is_f64() {
            self.as_f64() as i32
        } else if self.is_bool() {
            if self.as_bool() {
                1
            } else {
                0
            }
        } else {
            0
        }
    }
}

impl PartialEq for VmValue {
    fn eq(&self, other: &Self) -> bool {
        if self.0 == other.0 {
            return true;
        }
        if (self.is_int() || self.is_f64()) && (other.is_int() || other.is_f64()) {
            return self.to_f64() == other.to_f64();
        }
        false
    }
}

impl Eq for VmValue {}

impl fmt::Debug for VmValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_f64() {
            write!(f, "f64({})", self.as_f64())
        } else if self.is_int() {
            write!(f, "i64({})", self.as_int())
        } else if self.is_bool() {
            write!(f, "bool({})", self.as_bool())
        } else if self.is_null() {
            write!(f, "null")
        } else if self.is_sso() {
            let mut buf = [0u8; 5];
            let s = self.sso_as_str(&mut buf);
            write!(f, "sso({:?})", s)
        } else if self.is_heap() {
            write!(f, "heap[{}]", self.as_heap_idx())
        } else {
            write!(f, "nan(0x{:016x})", self.0)
        }
    }
}

impl fmt::Display for VmValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

use std::cell::UnsafeCell;
use std::rc::Rc;

use crate::register_meta::SlotKind;

/// Backing representation of a [`VmArray`]'s element buffer.
///
/// `#[repr(C, u8)]` pins a *defined* layout so the JIT can probe it: the
/// discriminant is a `u8` at offset 0, and each variant's `Vec` payload sits
/// at a fixed offset after the tag + alignment padding (8 on 64-bit targets).
/// The template and CLIF backends read the discriminant to guard their inline
/// fast paths and read the `Vec` words directly — see
/// `Heap::jit_array_layout` and the `discriminant() == Boxed` guards in the
/// JIT array paths.
///
/// A `VmArray` owns exactly one `Rc<UnsafeCell<ArrayRepr>>`, so every clone
/// and alias shares this single cell. An in-place migration (typed → `Boxed`
/// on a type-mismatched write) is therefore visible to *all* aliases and
/// never changes the array's identity — the `Rc` address (and thus the heap
/// index / `===` semantics / `Map` key) is preserved because only the cell's
/// *contents* are swapped, never the cell itself.
///
/// The variant is chosen from the *values*, not from a static type: a literal
/// picks its repr in [`VmArray::from_items`], and an array that starts empty
/// specializes on its first [`VmArray::push_vm`]. Nothing else can produce a
/// typed repr, and every typed repr is reversible — a mismatched write
/// migrates back to `Boxed` in place — so the representation is never
/// observable from the language.
#[repr(C, u8)]
#[derive(Debug)]
pub enum ArrayRepr {
    /// str / object / heterogeneous / `Dynamic` elements — NaN-boxed, as today.
    Boxed(Vec<VmValue>) = 0,
    /// `Array<int>` — raw `i64` buffer, holds no heap refs (GC skips it in A.2).
    I64(Vec<i64>) = 1,
    /// `Array<float>` — raw `f64` buffer, holds no heap refs.
    F64(Vec<f64>) = 2,
}

impl ArrayRepr {
    /// The `repr(C, u8)` discriminant (0 = Boxed, 1 = I64, 2 = F64). Matches
    /// the byte the JIT reads at offset 0 of the `ArrayRepr`.
    #[inline(always)]
    pub fn discriminant(&self) -> u8 {
        match self {
            ArrayRepr::Boxed(_) => 0,
            ArrayRepr::I64(_) => 1,
            ArrayRepr::F64(_) => 2,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        match self {
            ArrayRepr::Boxed(v) => v.len(),
            ArrayRepr::I64(v) => v.len(),
            ArrayRepr::F64(v) => v.len(),
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A reference-counted, interior-mutable array whose element storage is one
/// of three representations (see [`ArrayRepr`]). Identity is the `Rc` address;
/// see the type-level docs on `ArrayRepr` for the single-cell / migration
/// invariant.
#[derive(Clone, Debug)]
pub struct VmArray(pub Rc<UnsafeCell<ArrayRepr>>);

impl VmArray {
    // ---- constructors -----------------------------------------------------

    /// Boxed array from NaN-boxed values. This is the ubiquitous constructor
    /// every current call site uses; it keeps building `Boxed` arrays exactly
    /// as before A.1.
    #[inline(always)]
    pub fn new(items: Vec<VmValue>) -> Self {
        Self(Rc::new(UnsafeCell::new(ArrayRepr::Boxed(items))))
    }

    /// Empty `Boxed` array.
    #[inline(always)]
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// `Array<int>` backed by a raw `i64` buffer.
    #[inline(always)]
    pub fn new_i64(items: Vec<i64>) -> Self {
        Self(Rc::new(UnsafeCell::new(ArrayRepr::I64(items))))
    }

    /// `Array<float>` backed by a raw `f64` buffer. See [`Self::new_i64`].
    #[inline(always)]
    pub fn new_f64(items: Vec<f64>) -> Self {
        Self(Rc::new(UnsafeCell::new(ArrayRepr::F64(items))))
    }

    /// Array from boxed values, choosing the narrowest repr the values admit:
    /// all-int → `I64`, all-float → `F64`, anything else (mixed, empty, or a
    /// non-numeric element) → `Boxed`.
    ///
    /// The choice is made from the runtime values rather than a static type
    /// because it must hold for *every* producer — literals, natives, JSON,
    /// spreads — and because a static `Array<int>` still has to be verified
    /// element-wise before its elements can be stored raw. Unboxing here is
    /// exact: `from_int(as_int())` and `from_f64(as_f64())` are the identity
    /// on values that pass `is_int` / `is_f64`, so reading a typed element
    /// back reproduces the original `VmValue` bit for bit.
    ///
    /// The scan is one pass over data already being moved into the array, so
    /// it costs no allocation and no extra traversal order of growth.
    pub fn from_items(items: Vec<VmValue>) -> Self {
        if items.is_empty() {
            return Self::new(items);
        }
        if items.iter().all(|v| v.is_int()) {
            return Self::new_i64(items.iter().map(|v| v.as_int()).collect());
        }
        if items.iter().all(|v| v.is_f64()) {
            return Self::new_f64(items.iter().map(|v| v.as_f64()).collect());
        }
        Self::new(items)
    }

    // ---- repr access (internal) ------------------------------------------

    /// Shared view of the repr.
    ///
    /// SAFETY: as with the old `borrow`, callers must not create a `&mut`
    /// alias into the same cell while the returned reference is live. The VM
    /// is single-threaded and never re-enters an array mutation underneath a
    /// live read, so this holds by construction.
    #[inline(always)]
    pub fn repr(&self) -> &ArrayRepr {
        unsafe { &*self.0.get() }
    }

    /// Exclusive view of the repr. SAFETY: see [`Self::repr`]; no other live
    /// reference (shared or exclusive) into the same cell may exist.
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    fn repr_mut(&self) -> &mut ArrayRepr {
        unsafe { &mut *self.0.get() }
    }

    // ---- generic queries --------------------------------------------------

    /// The current representation's discriminant (0/1/2). Used by the JIT
    /// probe and by dispatch that must branch on element kind.
    #[inline(always)]
    pub fn discriminant(&self) -> u8 {
        self.repr().discriminant()
    }

    /// Element kind as a [`SlotKind`]: Boxed → `Dynamic`, I64 → `Int`,
    /// F64 → `Float`.
    #[inline(always)]
    pub fn element_slotkind(&self) -> SlotKind {
        match self.repr() {
            ArrayRepr::Boxed(_) => SlotKind::Dynamic,
            ArrayRepr::I64(_) => SlotKind::Int,
            ArrayRepr::F64(_) => SlotKind::Float,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.repr().len()
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.repr().is_empty()
    }

    // ---- Boxed-variant projections (legacy call sites) -------------------

    /// Boxed-variant vector, or `None` for a typed repr. Total and panic-free.
    #[inline(always)]
    pub fn as_boxed(&self) -> Option<&Vec<VmValue>> {
        match self.repr() {
            ArrayRepr::Boxed(v) => Some(v),
            _ => None,
        }
    }

    /// Mutable Boxed-variant vector, or `None` for a typed repr.
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub fn as_boxed_mut(&self) -> Option<&mut Vec<VmValue>> {
        match self.repr_mut() {
            ArrayRepr::Boxed(v) => Some(v),
            _ => None,
        }
    }

    /// Legacy `&Vec<VmValue>` projection for call sites that structurally only
    /// ever hold `Boxed` arrays (they built the array boxed, or reached it
    /// from a path where no typed repr exists — true of every site before
    /// Task A.4). A typed repr here is a bug, not a reachable state; the cold
    /// panic makes that explicit rather than corrupting memory. Sites that
    /// could see typed variants must use the total `*_vm` accessors instead.
    #[inline(always)]
    pub fn borrow(&self) -> &Vec<VmValue> {
        match self.repr() {
            ArrayRepr::Boxed(v) => v,
            _ => unreachable_typed("borrow"),
        }
    }

    /// Mutable counterpart of [`Self::borrow`]; same Boxed-only contract.
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub fn borrow_mut(&self) -> &mut Vec<VmValue> {
        match self.repr_mut() {
            ArrayRepr::Boxed(v) => v,
            _ => unreachable_typed("borrow_mut"),
        }
    }

    // ---- total VmValue-level accessors (work on any variant) -------------

    /// Read element `idx` as a `VmValue`, boxing on read from a typed repr.
    /// `None` if out of bounds.
    #[inline]
    pub fn get_vm(&self, idx: usize) -> Option<VmValue> {
        match self.repr() {
            ArrayRepr::Boxed(v) => v.get(idx).copied(),
            ArrayRepr::I64(v) => v.get(idx).map(|&n| VmValue::from_int(n)),
            ArrayRepr::F64(v) => v.get(idx).map(|&f| VmValue::from_f64(f)),
        }
    }

    /// Store `val` at `idx`. A type-mismatched store into a typed repr
    /// migrates the array to `Boxed` in place (see [`ArrayRepr`]) and then
    /// stores boxed. Returns `false` only when `idx` is out of bounds.
    ///
    /// The common `Boxed` case is a single repr projection + bounds check; the
    /// typed arms fall out of the match (releasing the borrow) before the cold
    /// migration re-borrows, so there is no double projection on the hot path.
    #[inline]
    pub fn set_vm(&self, idx: usize, val: VmValue) -> bool {
        {
            match self.repr_mut() {
                ArrayRepr::Boxed(v) => {
                    return if idx < v.len() {
                        v[idx] = val;
                        true
                    } else {
                        false
                    };
                }
                ArrayRepr::I64(v) => {
                    if idx >= v.len() {
                        return false;
                    }
                    if val.is_int() {
                        v[idx] = val.as_int();
                        return true;
                    }
                }
                ArrayRepr::F64(v) => {
                    if idx >= v.len() {
                        return false;
                    }
                    if val.is_f64() {
                        v[idx] = val.as_f64();
                        return true;
                    }
                }
            }
        }
        // Cold: type-mismatched store into a typed repr → migrate, store boxed.
        self.migrate_to_boxed()[idx] = val;
        true
    }

    /// Append `val`. A type-mismatched push into a typed repr migrates the
    /// array to `Boxed` in place, then pushes boxed. Single repr projection on
    /// the hot (`Boxed`/matching-typed) path.
    ///
    /// An *empty* `Boxed` array specializes to the pushed value's repr instead
    /// of staying boxed. This is what makes `let a = []` followed by
    /// `a.push(int)` — the way essentially every array in real code is built —
    /// end up with a raw buffer; without it only literals could ever be typed.
    /// It cannot lose information: the array holds no elements to reinterpret,
    /// and a later mismatched push migrates straight back to `Boxed`.
    #[inline]
    pub fn push_vm(&self, val: VmValue) {
        {
            match self.repr_mut() {
                // Only a NON-empty Boxed array pushes boxed here; the empty
                // case falls out to `push_repr_change` to specialize.
                ArrayRepr::Boxed(v) => {
                    if !v.is_empty() {
                        v.push(val);
                        return;
                    }
                }
                ArrayRepr::I64(v) => {
                    if val.is_int() {
                        v.push(val.as_int());
                        return;
                    }
                }
                ArrayRepr::F64(v) => {
                    if val.is_f64() {
                        v.push(val.as_f64());
                        return;
                    }
                }
            }
        }
        self.push_repr_change(val);
    }

    /// The two pushes that can change the representation, kept out of line so
    /// [`Self::push_vm`]'s hot path stays a single projection plus a branch:
    /// an empty `Boxed` array adopting the pushed value's repr (once per
    /// array), and a type-mismatched push into a typed repr migrating back to
    /// `Boxed`. Reached only when the match above fell through, so a `Boxed`
    /// repr here is necessarily empty.
    #[cold]
    fn push_repr_change(&self, val: VmValue) {
        if matches!(self.repr(), ArrayRepr::Boxed(_)) {
            if val.is_int() {
                *self.repr_mut() = ArrayRepr::I64(vec![val.as_int()]);
            } else if val.is_f64() {
                *self.repr_mut() = ArrayRepr::F64(vec![val.as_f64()]);
            } else {
                match self.repr_mut() {
                    ArrayRepr::Boxed(v) => v.push(val),
                    _ => unreachable!("checked Boxed above"),
                }
            }
            return;
        }
        self.migrate_to_boxed().push(val);
    }

    /// Remove and return the last element as a `VmValue` (boxing from typed
    /// reprs). `None` when empty. No migration — a pop never changes type.
    #[inline]
    pub fn pop_vm(&self) -> Option<VmValue> {
        match self.repr_mut() {
            ArrayRepr::Boxed(v) => v.pop(),
            ArrayRepr::I64(v) => v.pop().map(VmValue::from_int),
            ArrayRepr::F64(v) => v.pop().map(VmValue::from_f64),
        }
    }
    /// Box every element of a typed repr and swap the repr to `Boxed` through
    /// the *same* cell (identity preserved; all aliases observe the change).
    /// Returns a mutable view of the resulting `Boxed` vec. No-op if already
    /// `Boxed`. Cold: only reached on a type-mismatched typed write, which is
    /// itself unreachable before Task A.4.
    #[cold]
    #[allow(clippy::mut_from_ref)]
    fn migrate_to_boxed(&self) -> &mut Vec<VmValue> {
        {
            let repr = self.repr_mut();
            if !matches!(repr, ArrayRepr::Boxed(_)) {
                let boxed: Vec<VmValue> = match repr {
                    ArrayRepr::I64(v) => v.iter().map(|&n| VmValue::from_int(n)).collect(),
                    ArrayRepr::F64(v) => v.iter().map(|&f| VmValue::from_f64(f)).collect(),
                    ArrayRepr::Boxed(_) => unreachable!(),
                };
                *repr = ArrayRepr::Boxed(boxed);
            }
        }
        match self.repr_mut() {
            ArrayRepr::Boxed(v) => v,
            _ => unreachable!(),
        }
    }

    // ---- raw typed accessors (later phases; total, panic-free) -----------

    /// Element `idx` as a raw `i64`, or `None` for a non-`I64` repr / OOB.
    #[inline]
    pub fn get_i64(&self, idx: usize) -> Option<i64> {
        match self.repr() {
            ArrayRepr::I64(v) => v.get(idx).copied(),
            _ => None,
        }
    }

    /// Store raw `i64` at `idx`; `false` on a non-`I64` repr or OOB.
    #[inline]
    pub fn set_i64(&self, idx: usize, val: i64) -> bool {
        match self.repr_mut() {
            ArrayRepr::I64(v) if idx < v.len() => {
                v[idx] = val;
                true
            }
            _ => false,
        }
    }

    /// Element `idx` as a raw `f64`, or `None` for a non-`F64` repr / OOB.
    #[inline]
    pub fn get_f64(&self, idx: usize) -> Option<f64> {
        match self.repr() {
            ArrayRepr::F64(v) => v.get(idx).copied(),
            _ => None,
        }
    }

    /// Store raw `f64` at `idx`; `false` on a non-`F64` repr or OOB.
    #[inline]
    pub fn set_f64(&self, idx: usize, val: f64) -> bool {
        match self.repr_mut() {
            ArrayRepr::F64(v) if idx < v.len() => {
                v[idx] = val;
                true
            }
            _ => false,
        }
    }
}

/// Cold panic for a Boxed-only projection reached with a typed repr. Kept out
/// of line so the fast projection stays a single branch.
#[cold]
#[inline(never)]
fn unreachable_typed(op: &str) -> ! {
    panic!(
        "VmArray::{op} called on a non-Boxed repr — typed arrays are not \
         constructed before Task A.4; this projection is Boxed-only"
    )
}

impl PartialEq for VmArray {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for VmArray {}

impl std::hash::Hash for VmArray {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}