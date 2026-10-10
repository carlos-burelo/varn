#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeErrorKind {
    #[default]
    Error,
    IntegerOverflow,
    DivisionByZero,

    MatchError,
    TypeError,
}

impl RuntimeErrorKind {
    pub const ALL: [Self; 5] = [
        Self::Error,
        Self::IntegerOverflow,
        Self::DivisionByZero,
        Self::MatchError,
        Self::TypeError,
    ];

    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::IntegerOverflow => "IntegerOverflow",
            Self::DivisionByZero => "DivisionByZero",
            Self::MatchError => "MatchError",
            Self::TypeError => "TypeError",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeErrorKind;

    #[test]
    fn class_names_are_distinct() {
        let mut names: Vec<_> = RuntimeErrorKind::ALL
            .iter()
            .map(|k| k.class_name())
            .collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), RuntimeErrorKind::ALL.len());
    }
}
