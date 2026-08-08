use crate::document::TokenRecord;
use varn_checker::ScopeId as CheckerScopeId;

#[derive(Clone, Debug)]
pub struct PositionalIndex {
    /// Tokens sorted by start offset
    tokens_by_offset: Vec<TokenRecord>,
    /// Scopes sorted by start offset (offset, ScopeId)
    scopes_by_offset: Vec<(u32, CheckerScopeId)>,
}

impl PositionalIndex {
    pub fn build(
        tokens: &[TokenRecord],
        node_scopes: &rustc_hash::FxHashMap<u32, CheckerScopeId>,
    ) -> Self {
        let mut tokens_by_offset = tokens.to_vec();
        tokens_by_offset.sort_by_key(|t| t.offset);

        let mut scopes_by_offset: Vec<(u32, CheckerScopeId)> = node_scopes
            .iter()
            .map(|(&off, &scope)| (off, scope))
            .collect();
        scopes_by_offset.sort_by_key(|(off, _)| *off);

        Self {
            tokens_by_offset,
            scopes_by_offset,
        }
    }

    /// Fast O(log N) binary search for token at offset
    pub fn token_at_offset(&self, offset: u32) -> Option<&TokenRecord> {
        if self.tokens_by_offset.is_empty() {
            return None;
        }

        match self.tokens_by_offset.binary_search_by(|t| {
            if offset < t.offset {
                std::cmp::Ordering::Greater
            } else if offset >= t.offset + t.length {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        }) {
            Ok(idx) => Some(&self.tokens_by_offset[idx]),
            Err(_) => None,
        }
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
