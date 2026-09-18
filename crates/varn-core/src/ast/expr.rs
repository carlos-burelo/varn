use super::arena::{ExprId, StmtId};
use crate::source::SourceRange;
use crate::Atom;

pub type AstId = u32;

#[derive(Clone, Debug)]
pub enum ExprKind {
    IntLiteral {
        value: i64,
        raw: Atom,
    },
    FloatLiteral {
        value: f64,
        raw: Atom,
    },
    BigIntLiteral {
        raw: Atom,
    },
    DecimalLiteral {
        raw: Atom,
    },
    StrLiteral {
        value: String,
    },
    CharLiteral {
        value: char,
    },
    BoolLiteral {
        value: bool,
    },
    NullLiteral,
    RegexLiteral {
        pattern: String,
        flags: String,
    },
    Template {
        parts: Vec<TemplatePart>,
    },
    TaggedTemplate {
        tag: ExprId,
        template: ExprId,
    },
    Identifier {
        name: Atom,
    },
    /// A hole where an expression was expected but the source did not supply
    /// one — `g.` with nothing after the dot, `const x = ` with no initializer.
    ///
    /// The parser emits this instead of failing so the enclosing construct
    /// still parses and still binds its symbols; that is what lets the editor
    /// answer questions about half-typed code. The parser has already reported
    /// the syntax error, so the checker types this as `Dynamic` **silently** —
    /// a second diagnostic here would paint the file red while typing.
    ///
    /// The compiler must reject it before lowering: it is never executable.
    Missing,
    This,
    Super,
    Array {
        elements: Vec<ArrayEl>,
    },
    Object {
        properties: Vec<ObjectProp>,
    },
    Tuple {
        elements: Vec<ExprId>,
    },
    Record {
        properties: Vec<ObjectProp>,
    },
    Unary {
        op: super::operators::UnaryOp,
        prefix: bool,
        operand: ExprId,
    },
    Update {
        op: super::operators::UpdateOp,
        prefix: bool,
        operand: ExprId,
    },
    Binary {
        op: super::operators::BinaryOp,
        left: ExprId,
        right: ExprId,
    },
    Logical {
        op: super::operators::LogicalOp,
        left: ExprId,
        right: ExprId,
    },
    Assign {
        op: super::operators::AssignOp,
        target: ExprId,
        value: ExprId,
    },
    Conditional {
        test: ExprId,
        consequent: ExprId,
        alternate: ExprId,
    },
    Member {
        object: ExprId,
        property: ExprId,
        computed: bool,
        optional: bool,
    },
    Call {
        callee: ExprId,
        type_args: Vec<super::types::TypeNode>,
        args: Vec<Arg>,
        optional: bool,
    },
    New {
        callee: ExprId,
        type_args: Vec<super::types::TypeNode>,
        args: Vec<Arg>,
    },
    Function {
        fn_id: Option<Atom>,
        params: Vec<super::pattern::Param>,
        return_type: Option<super::types::TypeNode>,
        body: StmtId,
        is_async: bool,
        is_generator: bool,
    },
    Arrow {
        params: Vec<super::pattern::Param>,
        body: Box<ArrowBody>,
        is_async: bool,
        return_type: Option<super::types::TypeNode>,
    },
    Sequence {
        expressions: Vec<ExprId>,
    },
    Paren {
        expression: ExprId,
    },
    Await {
        argument: ExprId,
    },
    Spawn {
        argument: ExprId,
    },
    Yield {
        argument: Option<ExprId>,
        delegate: bool,
    },
    Spread {
        argument: ExprId,
    },
    Pipeline {
        left: ExprId,
        right: ExprId,
    },
    Range {
        start: ExprId,
        end: ExprId,
        inclusive: bool,
    },
    NonNull {
        expression: ExprId,
    },
    Try {
        expression: ExprId,
    },
    As {
        expression: ExprId,
        type_ann: super::types::TypeNode,
    },
    Satisfies {
        expression: ExprId,
        type_ann: super::types::TypeNode,
    },
    ClassExpr {
        declaration: Box<super::decl::ClassDecl>,
    },
    Match {
        subject: ExprId,
        cases: Vec<MatchCase>,
    },
    Is {
        expression: ExprId,
        type_ann: super::types::TypeNode,
    },
    With {
        object: ExprId,
        properties: Vec<ObjectProp>,
    },
    MetaAccess {
        target: ExprId,
        property: Atom,
    },
}

#[derive(Clone, Debug)]
pub enum ArrayEl {
    Expr(ExprId),
    Spread(ExprId),
    Hole,
}

#[derive(Clone, Debug)]
pub enum ObjectProp {
    Property {
        key: PropKey,
        value: ExprId,
        shorthand: bool,
        computed: bool,
        range: SourceRange,
    },
    Method {
        key: PropKey,
        params: Vec<super::pattern::Param>,
        body: StmtId,
        return_type: Option<super::types::TypeNode>,
        is_async: bool,
        is_generator: bool,
        range: SourceRange,
    },
    Getter {
        key: PropKey,
        body: StmtId,
        return_type: Option<super::types::TypeNode>,
        range: SourceRange,
    },
    Setter {
        key: PropKey,
        param: super::pattern::Param,
        body: StmtId,
        range: SourceRange,
    },
    Spread {
        argument: ExprId,
        range: SourceRange,
    },
}

impl ObjectProp {
    pub fn range(&self) -> &SourceRange {
        match self {
            ObjectProp::Property { range, .. } => range,
            ObjectProp::Method { range, .. } => range,
            ObjectProp::Getter { range, .. } => range,
            ObjectProp::Setter { range, .. } => range,
            ObjectProp::Spread { range, .. } => range,
        }
    }
}

#[derive(Clone, Debug)]
pub enum PropKey {
    Identifier(String),
    Str(String),
    Int(i64),
    Computed(ExprId),
}

#[derive(Clone, Debug)]
pub enum TemplatePart {
    Literal(String),
    Interpolation(ExprId),
}

#[derive(Clone, Debug)]
pub enum ArrowBody {
    Expr(ExprId),
    Block(StmtId),
}

#[derive(Clone, Debug)]
pub enum Arg {
    Positional(ExprId),
    Spread(ExprId),
    Named { label: String, value: ExprId },
}

#[derive(Clone, Debug)]
pub struct MatchCase {
    pub pattern: super::pattern::MatchPattern,
    pub guard: Option<ExprId>,
    pub body: MatchBody,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub enum MatchBody {
    Block(StmtId),
    Expr(ExprId),
}
