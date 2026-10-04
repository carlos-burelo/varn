use std::cell::UnsafeCell;
use std::ptr::NonNull;

use super::{ArrayRepr, BoxedElems, VmValue};
use crate::register_meta::SlotKind;

/// An interior-mutable array living in a heap cell, whose element storage is
/// one of three representations (see [`ArrayRepr`]). The cell owns the repr,
/// so this is a plain pointer, valid for as long as the array is reachable;
/// identity is that address.
#[derive(Clone, Copy, Debug)]
pub struct VmArray(NonNull<UnsafeCell<ArrayRepr>>);

impl ArrayRepr {
    /// Boxed array from `VmValue`s, kept boxed whatever they hold.
    #[inline(always)]
    pub fn boxed(items: Vec<VmValue>) -> Self {
        ArrayRepr::Boxed(BoxedElems::new(items))
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
    pub fn from_items(items: Vec<VmValue>) -> Self {
        Self::unboxed(&items).unwrap_or_else(|| Self::boxed(items))
    }

    pub fn from_slice(items: &[VmValue]) -> Self {
        Self::unboxed(items).unwrap_or_else(|| Self::boxed(items.to_vec()))
    }

    fn unboxed(items: &[VmValue]) -> Option<Self> {
        let first = *items.first()?;
        if first.is_int() && items.iter().all(|v| v.is_int()) {
            return Some(ArrayRepr::I64(items.iter().map(|v| v.as_int()).collect()));
        }
        if first.is_f64() && items.iter().all(|v| v.is_f64()) {
            return Some(ArrayRepr::F64(items.iter().map(|v| v.as_f64()).collect()));
        }
        None
    }
}

impl VmArray {
    /// Places `repr` at `at` and names it.
    ///
    /// # Safety
    /// `at` must be aligned for and point to `size_of::<ArrayRepr>()` writable
    /// bytes owned by the heap cell, which drops the repr when it frees.
    #[inline(always)]
    pub unsafe fn init_at(at: *mut ArrayRepr, repr: ArrayRepr) -> Self {
        std::ptr::write(at, repr);
        Self(NonNull::new_unchecked(at as *mut UnsafeCell<ArrayRepr>))
    }

    /// Drops the repr a cell holds: its element buffer.
    ///
    /// # Safety
    /// `self` must have come from [`Self::init_at`] and never be used again.
    pub unsafe fn drop_at(self) {
        std::ptr::drop_in_place(self.0.as_ptr());
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
        unsafe { &*(*self.0.as_ptr()).get() }
    }

    /// Exclusive view of the repr. SAFETY: see [`Self::repr`]; no other live
    /// reference (shared or exclusive) into the same cell may exist.
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub(super) fn repr_mut(&self) -> &mut ArrayRepr {
        unsafe { &mut *(*self.0.as_ptr()).get() }
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
            ArrayRepr::Boxed(v) => Some(&v.items),
            _ => None,
        }
    }

    /// Visits every element of a `Boxed` array that may still hold a young
    /// reference, then records the whole array as clean. For the minor
    /// collector only: every visited reference is old once it finishes.
    pub fn scan_dirty(&self, mut f: impl FnMut(VmValue)) {
        if let ArrayRepr::Boxed(b) = self.repr_mut() {
            let from = (b.clean_prefix as usize).min(b.items.len());
            for &v in &b.items[from..] {
                f(v);
            }
            b.clean_prefix = b.items.len() as u32;
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
            ArrayRepr::Boxed(v) => &v.items,
            _ => unreachable_typed("borrow"),
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
        self.0 == other.0
    }
}

impl Eq for VmArray {}

impl std::hash::Hash for VmArray {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.as_ptr().hash(state);
    }
}
