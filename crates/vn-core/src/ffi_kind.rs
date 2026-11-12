#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FfiKind {
    Void,
    Int,
    Float,
    Str,
    Ptr,
    Bool,
    Int32,
    Float32,
}

impl FfiKind {
    pub const ALL: [Self; 8] = [
        Self::Void,
        Self::Int,
        Self::Float,
        Self::Str,
        Self::Ptr,
        Self::Bool,
        Self::Int32,
        Self::Float32,
    ];

    pub const fn raw(self) -> i64 {
        match self {
            Self::Void => 0,
            Self::Int => 1,
            Self::Float => 2,
            Self::Str => 3,
            Self::Ptr => 4,
            Self::Bool => 5,
            Self::Int32 => 6,
            Self::Float32 => 7,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Void => "Void",
            Self::Int => "Int",
            Self::Float => "Float",
            Self::Str => "Str",
            Self::Ptr => "Ptr",
            Self::Bool => "Bool",
            Self::Int32 => "Int32",
            Self::Float32 => "Float32",
        }
    }

    pub fn of_raw(raw: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.raw() == raw)
    }
}
