use super::stmt::Stmt;
use crate::source::SourceRange;
use std::rc::Rc;

pub type AstId = u32;

#[derive(Clone, Debug)]
pub struct Expr {
    pub id: AstId,
    pub range: SourceRange,
    pub kind: ExprKind,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    IntLiteral {
        value: i64,
        raw: Rc<str>,
    },
    FloatLiteral {
        value: f64,
        raw: Rc<str>,
    },
    BigIntLiteral {
        raw: Rc<str>,
    },
    DecimalLiteral {
        raw: Rc<str>,
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
        tag: Box<Expr>,
        template: Box<Expr>,
    },
    Identifier {
        name: Rc<str>,
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
        elements: Vec<Expr>,
    },
    Record {
        properties: Vec<ObjectProp>,
    },
    Unary {
        op: super::operators::UnaryOp,
        prefix: bool,
        operand: Box<Expr>,
    },
    Update {
        op: super::operators::UpdateOp,
        prefix: bool,
        operand: Box<Expr>,
    },
    Binary {
        op: super::operators::BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Logical {
        op: super::operators::LogicalOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Assign {
        op: super::operators::AssignOp,
        target: Box<Expr>,
        value: Box<Expr>,
    },
    Conditional {
        test: Box<Expr>,
        consequent: Box<Expr>,
        alternate: Box<Expr>,
    },
    Member {
        object: Box<Expr>,
        property: Box<Expr>,
        computed: bool,
        optional: bool,
    },
    Call {
        callee: Box<Expr>,
        type_args: Vec<super::types::TypeNode>,
        args: Vec<Arg>,
        optional: bool,
    },
    New {
        callee: Box<Expr>,
        type_args: Vec<super::types::TypeNode>,
        args: Vec<Arg>,
    },
    Function {
        fn_id: Option<Rc<str>>,
        params: Vec<super::pattern::Param>,
        return_type: Option<super::types::TypeNode>,
        body: Box<Stmt>,
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
        expressions: Vec<Expr>,
    },
    Paren {
        expression: Box<Expr>,
    },
    Await {
        argument: Box<Expr>,
    },
    Spawn {
        argument: Box<Expr>,
    },
    Yield {
        argument: Option<Box<Expr>>,
        delegate: bool,
    },
    Spread {
        argument: Box<Expr>,
    },
    Pipeline {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    NonNull {
        expression: Box<Expr>,
    },
    Try {
        expression: Box<Expr>,
    },
    As {
        expression: Box<Expr>,
        type_ann: super::types::TypeNode,
    },
    Satisfies {
        expression: Box<Expr>,
        type_ann: super::types::TypeNode,
    },
    ClassExpr {
        declaration: Box<super::decl::ClassDecl>,
    },
    Match {
        subject: Box<Expr>,
        cases: Vec<MatchCase>,
    },
    Is {
        expression: Box<Expr>,
        type_ann: super::types::TypeNode,
    },
    With {
        object: Box<Expr>,
        properties: Vec<ObjectProp>,
    },
    MetaAccess {
        target: Box<Expr>,
        property: Rc<str>,
    },
}

#[derive(Clone, Debug)]
pub enum ArrayEl {
    Expr(Expr),
    Spread(Expr),
    Hole,
}

#[derive(Clone, Debug)]
pub enum ObjectProp {
    Property {
        key: PropKey,
        value: Expr,
        shorthand: bool,
        computed: bool,
        range: SourceRange,
    },
    Method {
        key: PropKey,
        params: Vec<super::pattern::Param>,
        body: Box<Stmt>,
        return_type: Option<super::types::TypeNode>,
        is_async: bool,
        is_generator: bool,
        range: SourceRange,
    },
    Getter {
        key: PropKey,
        body: Box<Stmt>,
        return_type: Option<super::types::TypeNode>,
        range: SourceRange,
    },
    Setter {
        key: PropKey,
        param: super::pattern::Param,
        body: Box<Stmt>,
        range: SourceRange,
    },
    Spread {
        argument: Expr,
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
    Computed(Expr),
}

#[derive(Clone, Debug)]
pub enum TemplatePart {
    Literal(String),
    Interpolation(Expr),
}

#[derive(Clone, Debug)]
pub enum ArrowBody {
    Expr(Expr),
    Block(Stmt),
}

#[derive(Clone, Debug)]
pub enum Arg {
    Positional(Expr),
    Spread(Expr),
    Named { label: String, value: Expr },
}

#[derive(Clone, Debug)]
pub struct MatchCase {
    pub pattern: super::pattern::MatchPattern,
    pub guard: Option<Expr>,
    pub body: MatchBody,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub enum MatchBody {
    Block(Stmt),
    Expr(Expr),
}

impl Expr {
    pub fn new(id: AstId, range: SourceRange, kind: ExprKind) -> Self {
        Self { id, range, kind }
    }

    pub fn id(&self) -> AstId {
        self.id
    }

    pub fn range(&self) -> &SourceRange {
        &self.range
    }
}
