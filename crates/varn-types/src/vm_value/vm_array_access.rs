use super::{ArrayRepr, BoxedElems, VmArray, VmValue};

impl VmArray {
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
                    return if idx < v.items.len() {
                        v.items[idx] = val;
                        v.clean_prefix = v.clean_prefix.min(idx as u32);
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
                    if !v.items.is_empty() {
                        v.items.push(val);
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
                    ArrayRepr::Boxed(v) => v.items.push(val),
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
            ArrayRepr::Boxed(v) => {
                let popped = v.items.pop();
                v.clean_prefix = v.clean_prefix.min(v.items.len() as u32);
                popped
            }
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
                *repr = ArrayRepr::Boxed(BoxedElems::new(boxed));
            }
        }
        match self.repr_mut() {
            ArrayRepr::Boxed(v) => {
                v.clean_prefix = 0;
                &mut v.items
            }
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
