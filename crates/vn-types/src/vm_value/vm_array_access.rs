use super::{ArrayRepr, BoxedElems, VmArray, VmValue};
use crate::memory::ExternalLedger;

#[inline(always)]
fn push_vec<T>(v: &mut Vec<T>, val: T) {
    let cap_before = v.capacity();
    let grows = v.len() == cap_before;
    v.push(val);
    if grows {
        ExternalLedger::grow((v.capacity() - cap_before) * std::mem::size_of::<T>());
    }
}

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
                        push_vec(&mut v.items, val);
                        return;
                    }
                }
                ArrayRepr::I64(v) => {
                    if val.is_int() {
                        push_vec(v, val.as_int());
                        return;
                    }
                }
                ArrayRepr::F64(v) => {
                    if val.is_f64() {
                        push_vec(v, val.as_f64());
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
                ExternalLedger::grow(std::mem::size_of::<i64>());
                *self.repr_mut() = ArrayRepr::I64(vec![val.as_int()]);
            } else if val.is_f64() {
                ExternalLedger::grow(std::mem::size_of::<f64>());
                *self.repr_mut() = ArrayRepr::F64(vec![val.as_f64()]);
            } else {
                match self.repr_mut() {
                    ArrayRepr::Boxed(v) => v.items.push(val),
                    ArrayRepr::I64(_) | ArrayRepr::F64(_) => unreachable!("checked Boxed above"),
                }
            }
            return;
        }
        self.migrate_to_boxed().push(val);
    }

    #[inline]
    pub fn pop_vm(&self) -> Option<VmValue> {
        match self.repr_mut() {
            ArrayRepr::Boxed(v) => v.items.pop(),
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
                ExternalLedger::grow(boxed.capacity() * std::mem::size_of::<VmValue>());
                *repr = ArrayRepr::Boxed(BoxedElems::new(boxed));
            }
        }
        match self.repr_mut() {
            ArrayRepr::Boxed(v) => &mut v.items,
            ArrayRepr::I64(_) | ArrayRepr::F64(_) => unreachable!(),
        }
    }

    #[inline]
    pub fn get_i64(&self, idx: usize) -> Option<i64> {
        match self.repr() {
            ArrayRepr::I64(v) => v.get(idx).copied(),
            ArrayRepr::Boxed(_) | ArrayRepr::F64(_) => None,
        }
    }

    #[inline]
    pub fn set_i64(&self, idx: usize, val: i64) -> bool {
        match self.repr_mut() {
            ArrayRepr::I64(v) if idx < v.len() => {
                v[idx] = val;
                true
            }
            ArrayRepr::Boxed(_) | ArrayRepr::I64(_) | ArrayRepr::F64(_) => false,
        }
    }

    #[inline]
    pub fn get_f64(&self, idx: usize) -> Option<f64> {
        match self.repr() {
            ArrayRepr::F64(v) => v.get(idx).copied(),
            ArrayRepr::Boxed(_) | ArrayRepr::I64(_) => None,
        }
    }

    #[inline]
    pub fn set_f64(&self, idx: usize, val: f64) -> bool {
        match self.repr_mut() {
            ArrayRepr::F64(v) if idx < v.len() => {
                v[idx] = val;
                true
            }
            ArrayRepr::Boxed(_) | ArrayRepr::I64(_) | ArrayRepr::F64(_) => false,
        }
    }
}
