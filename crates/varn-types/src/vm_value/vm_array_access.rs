use super::{ArrayRepr, BoxedElems, VmArray, VmValue};

impl VmArray {
    #[inline]
    pub fn get_vm(&self, idx: usize) -> Option<VmValue> {
        match self.repr() {
            ArrayRepr::Boxed(v) => v.get(idx).copied(),
            ArrayRepr::I64(v) => v.get(idx).map(|&n| VmValue::from_int(n)),
            ArrayRepr::F64(v) => v.get(idx).map(|&f| VmValue::from_f64(f)),
        }
    }

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

        self.migrate_to_boxed()[idx] = val;
        true
    }

    #[inline]
    pub fn push_vm(&self, val: VmValue) {
        {
            match self.repr_mut() {
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

    #[inline]
    pub fn get_i64(&self, idx: usize) -> Option<i64> {
        match self.repr() {
            ArrayRepr::I64(v) => v.get(idx).copied(),
            _ => None,
        }
    }

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

    #[inline]
    pub fn get_f64(&self, idx: usize) -> Option<f64> {
        match self.repr() {
            ArrayRepr::F64(v) => v.get(idx).copied(),
            _ => None,
        }
    }

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
