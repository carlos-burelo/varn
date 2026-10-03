use super::*;

impl Nursery {
    pub(super) fn scan_and_fix_old_obj(
        &mut self,
        raw_old: u32,
        old_gen: &mut HeapInner,
        worklist: &mut Vec<u32>,
        fixups: &mut Vec<(ChildSlot, u32)>,
    ) {
        fixups.clear();

        // ONE lookup, whatever the variant. This runs once per promoted
        // object, and probing `get_raw` separately for Generator, then Map,
        // then Set, then everything else charged four bounds-checked slot
        // reads to reach the common case (a plain Object or Array). The
        // three container variants still have to hand their handle out of
        // the borrow — they evacuate THROUGH `old_gen`, so its borrow must
        // end first — but every other variant is scanned right here.
        let deferred = match old_gen.get_raw(raw_old) {
            Some(HeapObj::Generator(g)) => Container::Generator(g.0.clone()),
            Some(HeapObj::Map(m)) => Container::Map(m.clone()),
            Some(HeapObj::Set(s)) => Container::Set(s.clone()),
            Some(HeapObj::Array(a) | HeapObj::Tuple(a)) => Container::Array(a.clone()),
            Some(HeapObj::Instance(inst)) => Container::Instance(inst.clone()),
            Some(HeapObj::Object(o) | HeapObj::Record(o)) => Container::Object(o.clone()),
            Some(HeapObj::Class(cls)) => Container::Class(cls.clone()),
            Some(obj) => {
                Self::scan_children(obj, fixups);
                Container::None
            }
            None => Container::None,
        };

        match deferred {
            // A generator carries a whole suspended ExecCtx (stack, frames,
            // upvalues, pending suspends) whose slots hold raw heap indices;
            // rewrite them in place through the driver's mutable trace.
            Container::Generator(driver) => {
                driver.trace_vm_values_mut(&mut |val| {
                    self.update_value(val, old_gen, worklist);
                });
                return;
            }
            Container::Array(arr) => {
                arr.scan_dirty(|v| self.update_value(v, old_gen, worklist));
                return;
            }
            Container::Instance(inst) => {
                inst.update_references(|val| self.update_value(val, old_gen, worklist));
                return;
            }
            Container::Object(o) => {
                let slot_count = o.slot_count();
                for i in 0..slot_count {
                    if let Some(mut v) = o.field_at(i) {
                        if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                            self.update_value(&mut v, old_gen, worklist);
                            o.set_field_at(i, v);
                        }
                    }
                }
                return;
            }
            // Map/Set entries are raw VmValues mutated through interior
            // mutability (the write barrier remembers the collection).
            // Values rewrite in place; canonical keys (interned strings,
            // scalars) are old-gen-stable, but identity keys can move —
            // their hash is their bit pattern, so the table is rebuilt when
            // any key evacuates.
            Container::Map(m) => {
                let mut g = m.borrow_mut();
                for v in g.values_mut() {
                    self.update_value(v, old_gen, worklist);
                }
                let any_key_moved = g
                    .keys()
                    .any(|k| k.0.is_heap() && is_nursery_idx(k.0.as_heap_idx()));
                if any_key_moved {
                    let entries: Vec<(VmValue, VmValue)> =
                        g.drain().map(|(k, v)| (k.0, v)).collect();
                    for (mut k, v) in entries {
                        self.update_value(&mut k, old_gen, worklist);
                        g.insert(varn_types::value::MapKey(k), v);
                    }
                }
                return;
            }
            Container::Set(s) => {
                let mut g = s.borrow_mut();
                let any_key_moved = g
                    .iter()
                    .any(|k| k.0.is_heap() && is_nursery_idx(k.0.as_heap_idx()));
                if any_key_moved {
                    let items: Vec<VmValue> = g.drain().map(|k| k.0).collect();
                    for mut k in items {
                        self.update_value(&mut k, old_gen, worklist);
                        g.insert(varn_types::value::MapKey(k));
                    }
                }
                return;
            }
            Container::Class(cls) => {
                cls.for_each_value_mut(&mut |v| self.update_value(v, old_gen, worklist));
                return;
            }
            Container::None => {}
        }

        for (slot, nursery_idx) in fixups.drain(..) {
            let packed = self.evacuate(nursery_idx, old_gen, worklist);
            let new_val = VmValue::from_heap_idx(packed);
            let Some(obj) = old_gen.get_raw_mut(raw_old) else {
                continue;
            };
            match (slot, obj) {
                (ChildSlot::Upvalue(i), HeapObj::VmClosure(clos)) => {
                    if let Some(uv) = clos.upvalues.get(i) {
                        if let Ok(mut inner) = uv.inner.try_borrow_mut() {
                            inner.value = new_val;
                        }
                    }
                }
                (ChildSlot::Spread, HeapObj::Spread(v)) => {
                    *v = new_val;
                }
                (ChildSlot::BoundMethodReceiver, HeapObj::BoundMethod(bm)) => {
                    bm.receiver = new_val;
                }
                (ChildSlot::BoundMethodClosure, HeapObj::BoundMethod(bm)) => {
                    if let varn_types::value::BoundMethodTarget::Vm { closure, .. } = &mut bm.target
                    {
                        *closure = new_val;
                    }
                }
                (ChildSlot::ModuleExport(i), HeapObj::Module(m)) => {
                    if let Some(s) = Rc::make_mut(m).exports.get_mut(i) {
                        *s = new_val;
                    }
                }
                (ChildSlot::EnumVariantPayload, HeapObj::EnumVariant(ev)) => {
                    ev.payload = new_val;
                }
                _ => {}
            }
        }
    }

    /// Record every child of `obj` that still points into the nursery. Pure
    /// scan: it only reads through the borrow of `old_gen` the caller holds,
    /// so the evacuation that consumes `fixups` happens after it returns.
    /// Whether [`Self::scan_children`] can produce a fixup for `obj` — i.e.
    /// whether this kind of object can hold a reference to a nursery object.
    ///
    /// An old-generation object is only visited by the minor collector if it is
    /// in `scan_roots` or was caught by the write barrier. An object BORN in the
    /// old generation — which happens whenever the nursery is full at the moment
    /// it is allocated — goes through neither, so it must be enrolled at birth,
    /// and this is the predicate that decides.
    ///
    /// It must agree with `scan_children` below. It did not: `Array`, `Object`,
    /// `Spread` and `EnumVariant` were handled there and missing here. A
    /// `JSON.parse` of 50 000 objects fills the nursery, the result array lands
    /// in the old generation holding nursery elements, and the next minor
    /// collection evacuates those elements without updating it — the array is
    /// left pointing at freed slots and the following read panics with
    /// "dangling or corrupted heap reference".
    ///
    /// Keep the two matches in the same file, and in the same order, so a new
    /// `HeapObj` variant cannot be added to one alone.
    pub(crate) fn can_reference_nursery(obj: &HeapObj) -> bool {
        matches!(
            obj,
            HeapObj::Array(_)
                | HeapObj::Tuple(_)
                | HeapObj::Object(_)
                | HeapObj::Instance(_)
                | HeapObj::Record(_)
                | HeapObj::VmClosure(_)
                | HeapObj::Spread(_)
                | HeapObj::BoundMethod(_)
                | HeapObj::Class(_)
                | HeapObj::Module(_)
                | HeapObj::EnumVariant(_)
                | HeapObj::Generator(_)
                | HeapObj::Map(_)
                | HeapObj::Set(_)
        )
    }

    /// Whether `obj` points at a nursery object right now.
    ///
    /// The write barrier catches an old-generation object being WRITTEN with a
    /// nursery reference, but nothing catches one being BORN holding them —
    /// which happens to every object allocated while the nursery is full. Those
    /// belong in `remembered` at birth; this is the test, applied once, at
    /// allocation. See `HeapInner::alloc`.
    pub(crate) fn holds_nursery_ref(obj: &HeapObj) -> bool {
        let nursery_val = |v: &VmValue| v.is_heap() && is_nursery_idx(v.as_heap_idx());
        match obj {
            HeapObj::Array(arr) | HeapObj::Tuple(arr) => match arr.as_boxed() {
                Some(items) => items.iter().any(nursery_val),
                None => false,
            },
            HeapObj::Instance(inst) => {
                let mut found = false;
                inst.for_each_reference(|v| found |= nursery_val(&v));
                found
            }
            HeapObj::Object(o) | HeapObj::Record(o) => {
                let mut found = false;
                o.borrow().for_each_field(|_, v| {
                    found |= nursery_val(&v);
                });
                found
            }
            HeapObj::Spread(v) => nursery_val(v),
            HeapObj::BoundMethod(bm) => {
                nursery_val(&bm.receiver)
                    || matches!(&bm.target,
                        varn_types::value::BoundMethodTarget::Vm { closure, .. } if nursery_val(closure))
            }
            HeapObj::EnumVariant(ev) => nursery_val(&ev.payload),
            HeapObj::Class(_) | HeapObj::Map(_) | HeapObj::Set(_) => true,
            _ => false,
        }
    }

    fn scan_children(obj: &HeapObj, fixups: &mut Vec<(ChildSlot, u32)>) {
        match obj {
            HeapObj::VmClosure(clos) => {
                for (i, uv) in clos.upvalues.iter().enumerate() {
                    if let Ok(inner) = uv.inner.try_borrow() {
                        if inner.value.is_heap() && is_nursery_idx(inner.value.as_heap_idx()) {
                            fixups.push((ChildSlot::Upvalue(i), inner.value.as_heap_idx()));
                        }
                    }
                }
            }
            HeapObj::Spread(v) => {
                if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                    fixups.push((ChildSlot::Spread, v.as_heap_idx()));
                }
            }
            HeapObj::BoundMethod(bm) => {
                if bm.receiver.is_heap() && is_nursery_idx(bm.receiver.as_heap_idx()) {
                    fixups.push((ChildSlot::BoundMethodReceiver, bm.receiver.as_heap_idx()));
                }
                if let varn_types::value::BoundMethodTarget::Vm { closure, .. } = &bm.target {
                    if closure.is_heap() && is_nursery_idx(closure.as_heap_idx()) {
                        fixups.push((ChildSlot::BoundMethodClosure, closure.as_heap_idx()));
                    }
                }
            }
            HeapObj::Module(m) => {
                for (i, &v) in m.exports.iter().enumerate() {
                    if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                        fixups.push((ChildSlot::ModuleExport(i), v.as_heap_idx()));
                    }
                }
            }
            HeapObj::EnumVariant(ev) => {
                if ev.payload.is_heap() && is_nursery_idx(ev.payload.as_heap_idx()) {
                    fixups.push((ChildSlot::EnumVariantPayload, ev.payload.as_heap_idx()));
                }
            }
            _ => {}
        }
    }
}

/// The variants `scan_and_fix_old_obj` cannot scan under the borrow that
/// found them: each rewrites its contents by evacuating THROUGH `old_gen`,
/// so the handle has to leave the borrow first. Everything else is scanned
/// in place by `scan_children` and reports `None` here.
enum Container {
    Generator(Rc<dyn varn_types::generator::GeneratorDriver>),
    Map(varn_types::value::MapRef),
    Set(varn_types::value::SetRef),
    Array(varn_types::VmArray),
    Object(varn_types::value::ObjRef),
    Instance(varn_types::value::InstanceRef),
    Class(std::rc::Rc<varn_types::ClassObj>),
    None,
}

#[derive(Clone)]
pub(super) enum ChildSlot {
    Upvalue(usize),
    Spread,
    BoundMethodReceiver,
    BoundMethodClosure,
    ModuleExport(usize),
    EnumVariantPayload,
}
