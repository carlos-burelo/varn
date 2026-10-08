use super::cells::CellSpace;
use super::obj::HeapObj;
use crate::value::VmValue;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_types::HeapRef;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reach {
    Minor,
    Major,
}

pub(crate) fn for_each_child(
    cells: &CellSpace,
    r: HeapRef,
    identity: &FxHashMap<usize, HeapRef>,
    reach: Reach,
    f: &mut impl FnMut(HeapRef),
) {
    if cells.header_kind(r) == varn_types::cell::CELL_KIND_INSTANCE {
        let inst = unsafe {
            varn_types::value::InstanceRef::from_data_ptr(
                (r.addr() as usize + varn_types::cell::INST_CELL_DATA_OFF) as *mut u8,
            )
        };
        let mut value = |v: VmValue| {
            if v.is_heap() {
                f(v.as_heap());
            }
        };
        inst.for_each_reference(&mut value);
        if let Some(cls) = varn_types::ClassObj::find_by_id(inst.class_id) {
            class_child(&cls, identity, &mut value);
        }
        return;
    }
    for_each_heap_child(cells.get(r), identity, reach, f)
}

fn for_each_heap_child(
    obj: &HeapObj,
    identity: &FxHashMap<usize, HeapRef>,
    reach: Reach,
    f: &mut impl FnMut(HeapRef),
) {
    let mut value = |v: VmValue| {
        if v.is_heap() {
            f(v.as_heap());
        }
    };
    match obj {
        HeapObj::Str(_)
        | HeapObj::Symbol(_)
        | HeapObj::Char(_)
        | HeapObj::BigInt(_)
        | HeapObj::Decimal(_)
        | HeapObj::Range(_)
        | HeapObj::NativeFn(_, _)
        | HeapObj::FrozenModule(_)
        | HeapObj::Buffer(_) => {}
        HeapObj::Module(m) => m.exports.iter().for_each(|&v| value(v)),
        HeapObj::Array(arr) | HeapObj::Tuple(arr) => match reach {
            Reach::Minor => arr.scan_dirty(value),
            Reach::Major => {
                if let Some(items) = arr.as_boxed() {
                    items.iter().for_each(|&v| value(v));
                }
            }
        },
        HeapObj::Object(obj_ref) | HeapObj::Record(obj_ref) => {
            let guard = obj_ref.borrow();
            guard.for_each_field(|_, v| value(v));
            if let Some(cls) = guard.class() {
                class_child(&cls, identity, &mut value);
            }
        }
        HeapObj::VmClosure(clos) => {
            for upval in &clos.upvalues {
                if let Ok(inner) = upval.inner.try_borrow() {
                    value(inner.value);
                }
            }
            clos.constants.iter().for_each(|&v| value(v));
        }
        HeapObj::Class(cls) => {
            cls.for_each_value_mut(&mut |v| value(*v));
            if let Some(parent) = cls.superclass.borrow().as_ref() {
                class_child(parent, identity, &mut value);
            }
        }
        HeapObj::Map(map_ref) => {
            for (k, v) in map_ref.0.borrow().iter() {
                value(k.0);
                value(*v);
            }
        }
        HeapObj::Set(set_ref) => set_ref.0.borrow().iter().for_each(|k| value(k.0)),
        HeapObj::BoundMethod(bm) => {
            value(bm.receiver);
            if let varn_types::value::BoundMethodTarget::Vm { closure, .. } = &bm.target {
                value(*closure);
            }
        }
        HeapObj::Spread(v) => value(*v),
        HeapObj::EnumVariant(ev) => value(ev.payload),
        HeapObj::Task(task) => task.trace_cells(&mut |c| value(c.get())),
        HeapObj::TaskHandle(cell) => cell.trace_cells(&mut |c| value(c.get())),
        HeapObj::Generator(gen) => {
            gen.0.trace_vm_values(&mut value);
            gen.0.trace_closures(&mut |ptr| {
                if let Some(&idx) = identity.get(&ptr) {
                    value(VmValue::from_heap(idx));
                }
            });
        }
    }
}

fn class_child(
    cls: &Rc<varn_types::ClassObj>,
    identity: &FxHashMap<usize, HeapRef>,
    value: &mut impl FnMut(VmValue),
) {
    if let Some(&idx) = identity.get(&(Rc::as_ptr(cls) as usize)) {
        value(VmValue::from_heap(idx));
    }
}
