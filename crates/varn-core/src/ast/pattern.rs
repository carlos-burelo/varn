use super::arena::ExprId;
use super::operators::Modifiers;
use super::types::TypeNode;
use crate::source::SourceRange;
use crate::Atom;

#[derive(Clone, Debug)]
pub enum Pattern {
    Identifier {
        name: Atom,
        type_ann: Option<TypeNode>,
        range: SourceRange,
    },
    Array {
        elements: Vec<Option<ArrayPatternEl>>,
        rest: Option<Box<Pattern>>,
        range: SourceRange,
    },
    Object {
        properties: Vec<ObjPatternProp>,
        rest: Option<Box<Pattern>>,
        range: SourceRange,
    },

    Assignment {
        left: Box<Pattern>,
        right: ExprId,
        range: SourceRange,
    },
    Rest {
        argument: Box<Pattern>,
        range: SourceRange,
    },
}

impl Pattern {
    pub fn range(&self) -> &SourceRange {
        match self {
            Pattern::Identifier { range, .. } => range,
            Pattern::Array { range, .. } => range,
            Pattern::Object { range, .. } => range,
            Pattern::Assignment { range, .. } => range,
            Pattern::Rest { range, .. } => range,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ArrayPatternEl {
    pub pattern: Pattern,
}

#[derive(Clone, Debug)]
pub struct ObjPatternProp {
    pub key: Atom,
    pub value: Pattern,
    pub shorthand: bool,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub pattern: Pattern,
    pub type_ann: Option<TypeNode>,
    pub default: Option<ExprId>,
    pub is_rest: bool,
    pub is_optional: bool,
    pub modifiers: Modifiers,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub enum MatchPattern {
    Wildcard,
    Literal(ExprId),
    Identifier(Atom),
    Record {
        fields: Vec<(Atom, Option<MatchPattern>)>,
        rest: bool,
    },
    Sequence(Vec<MatchPattern>),
    Type {
        type_name: Atom,
        binding: Option<Atom>,
    },

    EnumVariant {
        enum_name: Atom,
        variant_name: Atom,

        bindings: Vec<MatchBinding>,
    },
}

#[derive(Clone, Debug)]
pub struct MatchBinding {
    pub name: Atom,
    pub range: SourceRange,
}
