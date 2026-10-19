#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextualKeyword {
    Keyof,
    Infer,
    Satisfies,
    Meta,
}

impl ContextualKeyword {
    const ALL: [Self; 4] = [Self::Keyof, Self::Infer, Self::Satisfies, Self::Meta];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Keyof => "keyof",
            Self::Infer => "infer",
            Self::Satisfies => "satisfies",
            Self::Meta => "meta",
        }
    }

    pub fn of(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == text)
    }
}
