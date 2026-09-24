//! Clase de plataforma de un error nacido en el runtime. Se decide donde nace
//! el error y viaja con él; el `catch` la materializa sin re-derivarla del texto.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeErrorKind {
    #[default]
    Error,
    IntegerOverflow,
    DivisionByZero,
    /// A `match` used as a value where no arm matched.
    MatchError,
}

impl RuntimeErrorKind {
    pub const ALL: [Self; 4] = [
        Self::Error,
        Self::IntegerOverflow,
        Self::DivisionByZero,
        Self::MatchError,
    ];

    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::IntegerOverflow => "IntegerOverflow",
            Self::DivisionByZero => "DivisionByZero",
            Self::MatchError => "MatchError",
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
