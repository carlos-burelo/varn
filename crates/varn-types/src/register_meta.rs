#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Int,
    Float,
    Bool,
    Str,
    Ref,
    Dynamic,
}

#[derive(Debug, Clone)]
pub struct RegisterMeta {
    pub kind: SlotKind,
}
