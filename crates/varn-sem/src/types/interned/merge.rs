use super::table::CheckerTyTable;

impl CheckerTyTable {
    pub fn absorb(&mut self, other: &CheckerTyTable) {
        let shared_base = std::sync::Arc::ptr_eq(&self.base, &other.base);
        if !shared_base {
            for (k, v) in &other.base.entries {
                if !self.contains(*k) {
                    self.delta_entries.insert(*k, *v);
                }
            }
        }
        for (k, v) in &other.delta_entries {
            if !self.contains(*k) {
                self.delta_entries.insert(*k, *v);
            }
        }
        if !shared_base {
            for (k, v) in &other.base.lists {
                if self
                    .base
                    .lists
                    .get(k)
                    .or_else(|| self.delta_lists.get(k))
                    .is_none()
                {
                    self.delta_lists.insert(*k, v.clone());
                }
            }
        }
        for (k, v) in &other.delta_lists {
            if self
                .base
                .lists
                .get(k)
                .or_else(|| self.delta_lists.get(k))
                .is_none()
            {
                self.delta_lists.insert(*k, v.clone());
            }
        }
        if !shared_base {
            for (k, v) in &other.base.functions {
                if self
                    .base
                    .functions
                    .get(k)
                    .or_else(|| self.delta_functions.get(k))
                    .is_none()
                {
                    self.delta_functions.insert(*k, v.clone());
                }
            }
        }
        for (k, v) in &other.delta_functions {
            if self
                .base
                .functions
                .get(k)
                .or_else(|| self.delta_functions.get(k))
                .is_none()
            {
                self.delta_functions.insert(*k, v.clone());
            }
        }
        if !shared_base {
            for (k, v) in &other.base.object_members {
                if !self.contains_object_members(*k) {
                    self.delta_object_members.insert(*k, v.clone());
                }
            }
        }
        for (k, v) in &other.delta_object_members {
            if !self.contains_object_members(*k) {
                self.delta_object_members.insert(*k, v.clone());
            }
        }
        self.names.absorb(&other.names);
        self.maybe_freeze();
    }
}
