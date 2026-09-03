#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SlotKind {
    Int,
    Float,
    Bool,
    Str,
    Ref,
    Class(u32),
    Array(u32),
    Nullable(u32),
    Dynamic,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct RegisterMeta {
    pub kind: SlotKind,
}
