use super::*;

impl ObjData<[Cell<VmValue>]> {
    #[inline]
    pub fn get(&self, name: &str) -> Option<VmValue> {
        let shape = self.shape();
        let ordered = shape.ordered_names();
        if ordered.len() <= 4 {
            for (slot, prop_name) in ordered.iter().enumerate() {
                if prop_name.as_ref() == name {
                    return self.field_at(slot);
                }
            }
            return None;
        }
        let slot = shape.property_names.get(name).copied()?;
        self.field_at(slot)
    }

    /// Sets `name`, growing the object if it is not already in the shape.
    /// Growth spills to the overflow store: the allocation must not move,
    /// because `Value::Object` identity is its address.
    pub fn insert(&self, name: RuntimeString, value: VmValue) {
        if let Some(&slot) = self.shape().property_names.get(&name) {
            if self.set_field_at(slot, value) {
                return;
            }
            // Shape says the slot exists but no store backs it yet: the field
            // was added to the shape by a sibling object. Fall through and
            // extend the overflow up to it.
            let overflow = self.overflow_mut();
            overflow.resize(slot - self.inline_len() + 1, VmValue::null());
            overflow[slot - self.inline_len()] = value;
            return;
        }

        let new_shape = self.shape().transition(Arc::clone(&name));
        let slot = new_shape.property_names[&name];
        self.set_shape(new_shape);

        let base = self.inline_len();
        if slot < base {
            self.values[slot].set(value);
            return;
        }
        let overflow = self.overflow_mut();
        if slot - base >= overflow.len() {
            overflow.resize(slot - base + 1, VmValue::null());
        }
        overflow[slot - base] = value;
    }

    /// `delete obj.x`: rebuilds the shape without `name` and repacks the
    /// remaining fields into the same allocation.
    pub fn remove(&self, name: &str) -> Option<VmValue> {
        let removed_slot = *self.shape().property_names.get(name)?;
        let removed = self.field_at(removed_slot)?;

        // Kept in original slot order so the repacked object preserves the
        // field order the language exposes through `keys()`.
        let mut ordered: Vec<(RuntimeString, usize)> = self
            .shape()
            .property_names
            .iter()
            .filter(|(k, _)| k.as_ref() != name)
            .map(|(k, &slot)| (Arc::clone(k), slot))
            .collect();
        ordered.sort_unstable_by_key(|(_, slot)| *slot);

        let remaining: Vec<(RuntimeString, VmValue)> = ordered
            .into_iter()
            .map(|(k, slot)| (k, self.field_at(slot).unwrap_or(VmValue::null())))
            .collect();

        let mut new_shape = root_shape();
        for (k, _) in &remaining {
            new_shape = new_shape.transition(Arc::clone(k));
        }
        self.set_shape(new_shape);

        // Repack. The tail cannot shrink, so trailing slots are nulled rather
        // than left holding values the GC would keep alive.
        let base = self.inline_len();
        let overflow = self.overflow_mut();
        overflow.clear();
        for (i, (_, v)) in remaining.iter().enumerate() {
            if i < base {
                self.values[i].set(*v);
            } else {
                overflow.push(*v);
            }
        }
        for i in remaining.len()..base {
            self.values[i].set(VmValue::null());
        }

        Some(removed)
    }

    #[inline]
    pub fn contains_key(&self, name: &str) -> bool {
        let shape = self.shape();
        let ordered = shape.ordered_names();
        if ordered.len() <= 4 {
            return ordered.iter().any(|p| p.as_ref() == name);
        }
        shape.property_names.contains_key(name)
    }

    /// Number of fields the object exposes — the shape is the authority, not
    /// the tail, which keeps its size after a `remove`.
    #[inline]
    pub fn len(&self) -> usize {
        self.shape().property_names.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn keys(&self) -> std::vec::IntoIter<RuntimeString> {
        let mut pairs: Vec<(RuntimeString, usize)> = self
            .shape()
            .property_names
            .iter()
            .map(|(k, &idx)| (Arc::clone(k), idx))
            .collect();
        pairs.sort_unstable_by_key(|(_, idx)| *idx);
        pairs
            .into_iter()
            .map(|(k, _)| k)
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// The fields in declaration order, as owned pairs.
    ///
    /// Field order comes from [`Shape::ordered_names`], which the shape
    /// computed once. This used to sort a `Vec` built from the property
    /// HashMap and then collect it into a second `Vec` — two allocations, N
    /// `Rc` clones and a sort for every object visited, which `JSON.stringify`
    /// paid a million times over a 50 000-record document.
    ///
    /// Readers on a hot path should prefer walking `shape().ordered_names()`
    /// with [`Self::field_at`] directly: that allocates nothing at all.
    pub fn iter(&self) -> std::vec::IntoIter<(RuntimeString, VmValue)> {
        let names = self.shape().ordered_names();
        let mut pairs = Vec::with_capacity(names.len());
        for (slot, name) in names.iter().enumerate() {
            pairs.push((
                Arc::clone(name),
                self.field_at(slot).unwrap_or(VmValue::null()),
            ));
        }
        pairs.into_iter()
    }

    pub fn is_instance(&self) -> bool {
        self.shape().class.is_some()
    }

    pub fn class(&self) -> Option<Rc<ClassObj>> {
        self.shape().class.clone()
    }

    pub fn class_name(&self) -> String {
        match &self.shape().class {
            Some(c) => c.name.clone(),
            None => varn_core::RuntimeKind::Object.name().to_owned(),
        }
    }

    pub fn set_class(&self, class: Rc<ClassObj>) {
        let shape = Shape::create(Some(class), self.shape().property_names.clone());
        self.set_shape(shape);
    }

    #[inline]
    pub fn get_field(&self, key: &str) -> Option<VmValue> {
        self.get(key)
    }

    #[inline]
    pub fn set_field(&self, key: RuntimeString, value: VmValue) {
        self.insert(key, value);
    }

    #[inline]
    pub fn set_field_str(&self, key: &str, value: VmValue) {
        let shape = self.shape();
        let ordered = shape.ordered_names();
        let existing_slot = if ordered.len() <= 4 {
            ordered.iter().position(|p| p.as_ref() == key)
        } else {
            shape.property_names.get(key).copied()
        };
        if let Some(slot) = existing_slot {
            if self.set_field_at(slot, value) {
                return;
            }
            let overflow = self.overflow_mut();
            overflow.resize(slot - self.inline_len() + 1, VmValue::null());
            overflow[slot - self.inline_len()] = value;
            return;
        }

        let new_shape = self.shape().transition_str(key);
        let slot = new_shape.property_names[key];
        self.set_shape(new_shape);

        let base = self.inline_len();
        if slot < base {
            self.values[slot].set(value);
            return;
        }
        let overflow = self.overflow_mut();
        if slot - base >= overflow.len() {
            overflow.resize(slot - base + 1, VmValue::null());
        }
        overflow[slot - base] = value;
    }
}
