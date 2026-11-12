use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsaSpread {
    pub value: u32,
    pub spread: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsaObjectSpreadPart {
    pub key: Option<Box<str>>,
    pub value: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SsaUpvalue {
    Captured(u32),

    Inherited(u32),
}

pub const UPVALUE_LOCAL: u64 = 1 << 32;
