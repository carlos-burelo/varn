#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SlotKind {
    Int,
    Float,
    Bool,
    Str,
    Ref,
    Dynamic,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct RegisterMeta {
    pub kind: SlotKind,
}
