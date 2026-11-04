use super::arena::ExprId;
use super::operators::Modifiers;
use super::types::TypeNode;
use crate::source::SourceRange;
use crate::Atom;

#[derive(Clone, Debug)]
pub enum Pattern {
    Identifier {
        name: Atom,
        range: SourceRange,
    },
    Array {
        elements: Vec<Option<ArrayPatternEl>>,
        rest: Option<Box<Pattern>>,
        range: SourceRange,
    },
    Tuple {
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
            Pattern::Tuple { range, .. } => range,
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
    Tuple(Vec<MatchPattern>),
    Array {
        elements: Vec<MatchPattern>,
        rest: SequenceRest,
    },
    Record {
        fields: Vec<MatchField>,
        rest: bool,
    },
    EnumVariant {
        enum_name: Atom,
        variant_name: Atom,
        payload: VariantPayload,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequenceRest {
    Closed,
    Open,
    Bind(Atom),
}

#[derive(Clone, Debug)]
pub enum VariantPayload {
    Positional(Vec<MatchPattern>),
    Named { fields: Vec<MatchField>, rest: bool },
}

#[derive(Clone, Debug)]
pub struct MatchField {
    pub name: Atom,
    pub pattern: Option<MatchPattern>,
    pub range: SourceRange,
}

pub fn literal_variant_name(arena: &super::AstArena, expr: ExprId) -> Option<Atom> {
    use super::ExprKind;
    match &arena.expr(expr).kind {
        ExprKind::Member { property, .. } => match &arena.expr(*property).kind {
            ExprKind::Identifier { name } => Some(*name),
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::ImportMeta(_)
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => None,
        },
        ExprKind::Identifier { name } => Some(*name),
        ExprKind::IntLiteral { .. }
        | ExprKind::FloatLiteral { .. }
        | ExprKind::BigIntLiteral { .. }
        | ExprKind::DecimalLiteral { .. }
        | ExprKind::StrLiteral { .. }
        | ExprKind::CharLiteral { .. }
        | ExprKind::BoolLiteral { .. }
        | ExprKind::NullLiteral
        | ExprKind::Template { .. }
        | ExprKind::TaggedTemplate { .. }
        | ExprKind::Missing
        | ExprKind::This
        | ExprKind::Super
        | ExprKind::ImportMeta(_)
        | ExprKind::Array { .. }
        | ExprKind::Object { .. }
        | ExprKind::Tuple { .. }
        | ExprKind::Unary { .. }
        | ExprKind::Update { .. }
        | ExprKind::Binary { .. }
        | ExprKind::Logical { .. }
        | ExprKind::Assign { .. }
        | ExprKind::Conditional { .. }
        | ExprKind::Call { .. }
        | ExprKind::New { .. }
        | ExprKind::Function { .. }
        | ExprKind::Arrow { .. }
        | ExprKind::Sequence { .. }
        | ExprKind::Paren { .. }
        | ExprKind::Await { .. }
        | ExprKind::Spawn { .. }
        | ExprKind::Yield { .. }
        | ExprKind::Spread { .. }
        | ExprKind::Pipeline { .. }
        | ExprKind::Range { .. }
        | ExprKind::NonNull { .. }
        | ExprKind::Try { .. }
        | ExprKind::As { .. }
        | ExprKind::Satisfies { .. }
        | ExprKind::ClassExpr { .. }
        | ExprKind::Match { .. }
        | ExprKind::Is { .. }
        | ExprKind::With { .. }
        | ExprKind::MetaAccess { .. } => None,
    }
}
