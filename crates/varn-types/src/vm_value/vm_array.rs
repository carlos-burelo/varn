use std::cell::UnsafeCell;
use std::ptr::NonNull;

use super::{ArrayRepr, BoxedElems, VmValue};
use crate::register_meta::SlotKind;

#[derive(Clone, Copy, Debug)]
pub struct VmArray(NonNull<UnsafeCell<ArrayRepr>>);

impl ArrayRepr {
    #[inline(always)]
    pub fn boxed(items: Vec<VmValue>) -> Self {
        ArrayRepr::Boxed(BoxedElems::new(items))
    }

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
    #[inline(always)]
    pub unsafe fn init_at(at: *mut ArrayRepr, repr: ArrayRepr) -> Self {
        std::ptr::write(at, repr);
        Self(NonNull::new_unchecked(at as *mut UnsafeCell<ArrayRepr>))
    }

    pub unsafe fn drop_at(self) {
        std::ptr::drop_in_place(self.0.as_ptr());
    }

    #[inline(always)]
    pub fn repr(&self) -> &ArrayRepr {
        unsafe { &*(*self.0.as_ptr()).get() }
    }

    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub(super) fn repr_mut(&self) -> &mut ArrayRepr {
        unsafe { &mut *(*self.0.as_ptr()).get() }
    }

    #[inline(always)]
    pub fn discriminant(&self) -> u8 {
        self.repr().discriminant()
    }

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

    #[inline(always)]
    pub fn as_boxed(&self) -> Option<&Vec<VmValue>> {
        match self.repr() {
            ArrayRepr::Boxed(v) => Some(&v.items),
            ArrayRepr::I64(_) | ArrayRepr::F64(_) => None,
        }
    }

    pub fn scan_dirty(&self, mut f: impl FnMut(VmValue)) {
        if let ArrayRepr::Boxed(b) = self.repr_mut() {
            let from = (b.clean_prefix as usize).min(b.items.len());
            for &v in &b.items[from..] {
                f(v);
            }
            b.clean_prefix = b.items.len() as u32;
        }
    }

    #[inline(always)]
    pub fn borrow(&self) -> &Vec<VmValue> {
        match self.repr() {
            ArrayRepr::Boxed(v) => &v.items,
            ArrayRepr::I64(_) | ArrayRepr::F64(_) => unreachable_typed("borrow"),
        }
    }
}

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
