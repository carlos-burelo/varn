use super::table::CheckerTyTable;

impl CheckerTyTable {
    /// Union every shape in `other` into `self`. Commutative and idempotent:
    /// content-addressed ids mean a shape present in both tables has the same
    /// id, so this is a set union with no remap. This replaces the old
    /// `reintern`-based `absorb` (ADR-0012).
    pub fn absorb(&mut self, other: &CheckerTyTable) {
        // Skip the (redundant) base-vs-base walk when both tables already
        // share the same frozen base: every entry `other.base` could offer
        // is already in `self.base` by definition of sharing the `Arc`, so
        // only `other`'s delta can possibly be new to `self`.
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
        self.maybe_freeze();
    }
}
