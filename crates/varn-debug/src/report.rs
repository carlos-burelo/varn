








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



#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    None,
    Text(String),
    Rows(Vec<Vec<String>>),
    Tree(Vec<TreeNode>),
}
