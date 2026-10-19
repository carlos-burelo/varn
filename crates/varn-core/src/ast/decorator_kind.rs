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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractDecorator {
    Fallible,
    Permission,
}

impl ContractDecorator {
    const ALL: [Self; 2] = [Self::Fallible, Self::Permission];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Fallible => "fallible",
            Self::Permission => "permission",
        }
    }

    pub fn of(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecoratorTarget {
    Function,
    Method,
    Class,
    Getter,
    Setter,
    Constructor,
    Property,
}

impl DecoratorTarget {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Function => crate::TokenKind::Function.as_str(),
            Self::Method => "method",
            Self::Class => crate::TokenKind::Class.as_str(),
            Self::Getter => "getter",
            Self::Setter => "setter",
            Self::Constructor => crate::TokenKind::Constructor.as_str(),
            Self::Property => "property",
        }
    }

    pub const fn arity(self) -> usize {
        match self {
            Self::Function | Self::Class => 1,
            Self::Method | Self::Getter | Self::Setter | Self::Constructor | Self::Property => 2,
        }
    }

    pub const fn returns_value(self) -> bool {
        !matches!(self, Self::Property)
    }
}
