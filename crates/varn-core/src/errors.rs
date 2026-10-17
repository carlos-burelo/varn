#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeErrorKind {
    #[default]
    Error,
    IntegerOverflow,
    DivisionByZero,

    MatchError,
    TypeError,
    RangeError,
}

impl RuntimeErrorKind {
    pub const ALL: [Self; 6] = [
        Self::Error,
        Self::IntegerOverflow,
        Self::DivisionByZero,
        Self::MatchError,
        Self::TypeError,
        Self::RangeError,
    ];

    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::IntegerOverflow => "IntegerOverflow",
            Self::DivisionByZero => "DivisionByZero",
            Self::MatchError => "MatchError",
            Self::TypeError => "TypeError",
            Self::RangeError => "RangeError",
        }
    }

    pub const MODULE: &'static str = "core:errors";

    pub fn identify(name: crate::Atom, origin: Option<crate::Atom>) -> Option<Self> {
        let origin = origin?;
        if origin != crate::Atom::of_const(Self::MODULE) {
            return None;
        }
        Self::ALL
            .into_iter()
            .find(|k| crate::Atom::of_const(k.class_name()) == name)
    }
}
