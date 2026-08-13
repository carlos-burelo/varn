use varn_checker::ScopeId as CheckerScopeId;

#[derive(Clone, Debug)]
pub struct PositionalIndex {
    /// Scopes sorted by start offset (offset, ScopeId)
    scopes_by_offset: Vec<(u32, CheckerScopeId)>,
}

impl PositionalIndex {
    pub fn build(node_scopes: &rustc_hash::FxHashMap<u32, CheckerScopeId>) -> Self {
        let mut scopes_by_offset: Vec<(u32, CheckerScopeId)> = node_scopes
            .iter()
            .map(|(&off, &scope)| (off, scope))
            .collect();
        scopes_by_offset.sort_by_key(|(off, _)| *off);

        Self { scopes_by_offset }
    }

    /// Fast O(log N) binary search for nearest scope at offset
    pub fn scope_at_offset(&self, offset: u32, global_scope: CheckerScopeId) -> CheckerScopeId {
        if self.scopes_by_offset.is_empty() {
            return global_scope;
        }

        match self
            .scopes_by_offset
            .binary_search_by(|(off, _)| off.cmp(&offset))
        {
            Ok(idx) => self.scopes_by_offset[idx].1,
            Err(idx) => {
                if idx == 0 {
                    global_scope
                } else {
                    self.scopes_by_offset[idx - 1].1
                }
            }
        }
    }
}
