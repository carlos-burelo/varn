use super::decorators::BuiltinDecorator;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinDecoratorKind {
    Deprecated,
    Pure,
    Inline,
    Capability,
    Test,
}

impl BuiltinDecoratorKind {
    const ALL: [Self; 5] = [
        Self::Deprecated,
        Self::Pure,
        Self::Inline,
        Self::Capability,
        Self::Test,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Deprecated => "deprecated",
            Self::Pure => "pure",
            Self::Inline => "inline",
            Self::Capability => "capability",
            Self::Test => "test",
        }
    }

    pub fn of(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }
}

impl BuiltinDecorator {
    pub fn kind(&self) -> BuiltinDecoratorKind {
        match self {
            Self::Deprecated { .. } => BuiltinDecoratorKind::Deprecated,
            Self::Pure => BuiltinDecoratorKind::Pure,
            Self::Inline => BuiltinDecoratorKind::Inline,
            Self::Capability { .. } => BuiltinDecoratorKind::Capability,
            Self::Test => BuiltinDecoratorKind::Test,
        }
    }
}
