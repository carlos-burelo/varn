use super::*;

impl ExecCtx {
    pub(super) fn host_map_key(
        &mut self,
        v: VmValue,
    ) -> Result<varn_types::value::MapKey, varn_types::NativeError> {
        let v = self.hashable_key(v)?;
        Ok(self.heap.canonical_map_key(v))
    }
    pub(super) fn host_is_object(&self, v: VmValue) -> bool {
        if v.is_heap() {
            matches!(
                self.heap.get(v.as_heap()),
                Some(HeapObj::Object(_) | HeapObj::Record(_))
            )
        } else {
            false
        }
    }
    pub(super) fn host_object_for_each(&self, obj: VmValue, f: &mut dyn FnMut(&str, VmValue)) {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o) | HeapObj::Record(o)) = self.heap.get(obj.as_heap()) {
                for (k, v) in o.borrow().iter() {
                    f(k.as_ref(), v);
                }
            }
        }
    }
    pub(super) fn host_map_for_each(&self, map: VmValue, f: &mut dyn FnMut(VmValue, VmValue)) {
        if map.is_heap() {
            if let Some(HeapObj::Map(m)) = self.heap.get(map.as_heap()) {
                for (k, v) in m.0.borrow().iter() {
                    f(k.0, *v);
                }
            }
        }
    }
    pub(super) fn host_get_object_shape(
        &self,
        obj: VmValue,
    ) -> Option<std::rc::Rc<varn_types::Shape>> {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o) | HeapObj::Record(o)) = self.heap.get(obj.as_heap()) {
                return Some(std::rc::Rc::clone(o.borrow().shape()));
            }
        }
        None
    }
    pub(super) fn host_get_field(&self, obj: VmValue, key: &str) -> Option<VmValue> {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o) | HeapObj::Record(o)) = self.heap.get(obj.as_heap()) {
                return o.borrow().get_field(key);
            }

            if let Some(HeapObj::Instance(inst)) = self.heap.get(obj.as_heap()) {
                let cls = ClassObj::find_by_id(inst.class_id)?;
                let layout = cls.layout();
                let f = layout.get_field(key)?;
                return inst.read_field(f);
            }

            if let Some(HeapObj::Module(m)) = self.heap.get(obj.as_heap()) {
                let slot = m.export_map.get(key).copied()?;
                return m.get_slot(slot);
            }
        }
        None
    }
    pub(super) fn host_set_field(&mut self, obj: VmValue, key: &str, val: VmValue) {
        if obj.is_heap() {
            let idx = obj.as_heap();
            if let Some(HeapObj::Object(o)) = self.heap.get(idx) {
                o.set_field(std::sync::Arc::from(key), val);
                self.heap.write_barrier(idx, val);
            } else if let Some(HeapObj::Instance(inst)) = self.heap.get(idx) {
                let inst = *inst;
                let field = ClassObj::find_by_id(inst.class_id)
                    .map(|cls| cls.layout())
                    .and_then(|layout| layout.get_field(key).cloned());
                if let Some(f) = field {
                    if inst.write_field(&f, val).is_ok() {
                        self.heap.write_barrier(idx, val);
                    }
                }
            } else if let Some(HeapObj::Module(m)) = self.heap.get_mut(idx) {
                if let Some(s) = m.export_map.get(key).copied() {
                    std::rc::Rc::make_mut(m).set_slot(s, val);
                } else {
                    let m = std::rc::Rc::make_mut(m);
                    let slot = m.exports.len();
                    m.exports.push(val);
                    m.export_map.insert(std::sync::Arc::from(key), slot);
                }
            }
        }
    }
    pub(super) fn host_alloc_map(&mut self, entries: Vec<(VmValue, VmValue)>) -> VmValue {
        let mut map = varn_types::value::ValueMap::default();
        for (key, value) in entries {
            let key = self.map_key(key).unwrap_or(varn_types::value::MapKey(key));
            map.insert(key, value);
        }
        self.heap.alloc_map_vm(map)
    }
    pub(super) fn host_alloc_set(&mut self, items: Vec<VmValue>) -> VmValue {
        let mut set = varn_types::value::ValueSet::default();
        for item in items {
            let key = self
                .map_key(item)
                .unwrap_or(varn_types::value::MapKey(item));
            set.insert(key);
        }
        self.heap.alloc_set_vm(set)
    }
    pub(super) fn host_alloc_instance(&mut self, class_name: &str) -> Option<VmValue> {
        let class_obj = self.get_class(class_name)?;
        let instance_nv = self.heap.alloc_object();
        if let Some(crate::heap::HeapObj::Object(o)) = self.heap.get_mut(instance_nv.as_heap()) {
            o.set_class(class_obj);
        }
        Some(instance_nv)
    }
}
