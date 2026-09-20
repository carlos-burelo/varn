//! Phase-collected, format-agnostic intermediate representation
//! (DEBUG_PLAN §3.5).
//!
//! A phase's `collect` returns one of these; `render` turns it into either
//! `Plain` (byte-identical to the historical output) or `Text`. Keeping the
//! data separate from the printing is what makes `Text` possible without
//! duplicating every phase.

/// One node of a tree report (`ast`, `scope`, ...).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeNode {
    pub label: String,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            children: Vec::new(),
        }
    }

    pub fn child(mut self, child: TreeNode) -> Self {
        self.children.push(child);
        self
    }

    pub fn push(&mut self, child: TreeNode) {
        self.children.push(child);
    }
}

/// What a phase produced. `None` means "nothing to show" (e.g. a sweep phase
/// with no violations, like `clif:check`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    None,
    Text(String),
    Rows(Vec<Vec<String>>),
    Tree(Vec<TreeNode>),
}
