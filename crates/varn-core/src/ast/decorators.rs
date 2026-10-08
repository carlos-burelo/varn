use super::arena::{AstArena, ExprId};
use super::expr::{Arg, ExprKind};
use super::types::Decorator;
use crate::atom::{Atom, AtomInterner};
use crate::source::SourceRange;

pub enum BuiltinDecorator {
    Deprecated { message: Option<String> },
    Pure,
    Inline,
    Capability { domains: Vec<String> },
    Test,
}

pub struct BuiltinMatch {
    pub range: SourceRange,
    pub result: Option<Result<BuiltinDecorator, &'static str>>,
}

pub fn decorator_head(arena: &AstArena, interner: &AtomInterner, d: &Decorator) -> Option<Atom> {
    let head = match &arena.expr(d.expression).kind {
        ExprKind::Identifier { name } => *name,
        ExprKind::Call { callee, .. } => match &arena.expr(*callee).kind {
            ExprKind::Identifier { name } => *name,
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
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
            | ExprKind::MetaAccess { .. } => return None,
        },
        ExprKind::IntLiteral { .. }
        | ExprKind::FloatLiteral { .. }
        | ExprKind::BigIntLiteral { .. }
        | ExprKind::DecimalLiteral { .. }
        | ExprKind::StrLiteral { .. }
        | ExprKind::CharLiteral { .. }
        | ExprKind::BoolLiteral { .. }
        | ExprKind::NullLiteral
        | ExprKind::RegexLiteral { .. }
        | ExprKind::Template { .. }
        | ExprKind::TaggedTemplate { .. }
        | ExprKind::Missing
        | ExprKind::This
        | ExprKind::Super
        | ExprKind::Array { .. }
        | ExprKind::Object { .. }
        | ExprKind::Tuple { .. }
        | ExprKind::Record { .. }
        | ExprKind::Unary { .. }
        | ExprKind::Update { .. }
        | ExprKind::Binary { .. }
        | ExprKind::Logical { .. }
        | ExprKind::Assign { .. }
        | ExprKind::Conditional { .. }
        | ExprKind::Member { .. }
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
        | ExprKind::MetaAccess { .. } => return None,
    };
    match interner.resolve(head) {
        "deprecated" | "pure" | "inline" | "capability" | "test" => Some(head),
        _ => None,
    }
}

pub fn is_builtin(arena: &AstArena, interner: &AtomInterner, d: &Decorator) -> bool {
    match_builtin(arena, interner, std::slice::from_ref(d))
        .into_iter()
        .any(|m| m.result.is_some())
}

pub fn is_active_builtin(
    arena: &AstArena,
    interner: &AtomInterner,
    is_shadowed: impl Fn(u32) -> bool,
    d: &Decorator,
) -> bool {
    is_builtin(arena, interner, d) && !is_shadowed(d.range.start.offset)
}

fn str_arg(arena: &AstArena, id: ExprId) -> Option<String> {
    match &arena.expr(id).kind {
        ExprKind::StrLiteral { value } => Some(value.clone()),
        ExprKind::IntLiteral { .. }
        | ExprKind::FloatLiteral { .. }
        | ExprKind::BigIntLiteral { .. }
        | ExprKind::DecimalLiteral { .. }
        | ExprKind::CharLiteral { .. }
        | ExprKind::BoolLiteral { .. }
        | ExprKind::NullLiteral
        | ExprKind::RegexLiteral { .. }
        | ExprKind::Template { .. }
        | ExprKind::TaggedTemplate { .. }
        | ExprKind::Identifier { .. }
        | ExprKind::Missing
        | ExprKind::This
        | ExprKind::Super
        | ExprKind::Array { .. }
        | ExprKind::Object { .. }
        | ExprKind::Tuple { .. }
        | ExprKind::Record { .. }
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
    }
}

fn positional_strs(arena: &AstArena, args: &[Arg]) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        let Arg::Positional(e) = a else {
            return None;
        };
        out.push(str_arg(arena, *e)?);
    }
    Some(out)
}

pub fn match_builtin(
    arena: &AstArena,
    interner: &AtomInterner,
    decorators: &[Decorator],
) -> Vec<BuiltinMatch> {
    let mut out = Vec::new();
    for d in decorators {
        let (head, args) = match &arena.expr(d.expression).kind {
            ExprKind::Identifier { name } => (interner.resolve(*name), None),
            ExprKind::Call { callee, args, .. } => match &arena.expr(*callee).kind {
                ExprKind::Identifier { name } => (interner.resolve(*name), Some(args.as_slice())),
                ExprKind::IntLiteral { .. }
                | ExprKind::FloatLiteral { .. }
                | ExprKind::BigIntLiteral { .. }
                | ExprKind::DecimalLiteral { .. }
                | ExprKind::StrLiteral { .. }
                | ExprKind::CharLiteral { .. }
                | ExprKind::BoolLiteral { .. }
                | ExprKind::NullLiteral
                | ExprKind::RegexLiteral { .. }
                | ExprKind::Template { .. }
                | ExprKind::TaggedTemplate { .. }
                | ExprKind::Missing
                | ExprKind::This
                | ExprKind::Super
                | ExprKind::Array { .. }
                | ExprKind::Object { .. }
                | ExprKind::Tuple { .. }
                | ExprKind::Record { .. }
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
                | ExprKind::MetaAccess { .. } => {
                    out.push(BuiltinMatch {
                        range: d.range,
                        result: None,
                    });
                    continue;
                }
            },
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
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
            | ExprKind::MetaAccess { .. } => {
                out.push(BuiltinMatch {
                    range: d.range,
                    result: None,
                });
                continue;
            }
        };
        let result = match head {
            "deprecated" => match args {
                None | Some([]) => Ok(BuiltinDecorator::Deprecated { message: None }),
                Some([single]) => match single {
                    Arg::Positional(e) => match str_arg(arena, *e) {
                        Some(msg) => Ok(BuiltinDecorator::Deprecated { message: Some(msg) }),
                        None => Err("`@deprecated` takes an optional string message"),
                    },
                    Arg::Spread(_) | Arg::Named { .. } => {
                        Err("`@deprecated` takes an optional string message")
                    }
                },
                Some(_) => Err("`@deprecated` takes an optional string message"),
            },
            "pure" => match args {
                None | Some([]) => Ok(BuiltinDecorator::Pure),
                Some(_) => Err("`@pure` takes no arguments"),
            },
            "inline" => match args {
                None | Some([]) => Ok(BuiltinDecorator::Inline),
                Some(_) => Err("`@inline` takes no arguments"),
            },
            "capability" => match args {
                Some(a) if !a.is_empty() => match positional_strs(arena, a) {
                    Some(domains) => Ok(BuiltinDecorator::Capability { domains }),
                    None => Err("`@capability` takes one or more string domains"),
                },
                Some(_) | None => Err("`@capability` takes one or more string domains"),
            },
            "test" => match args {
                None | Some([]) => Ok(BuiltinDecorator::Test),
                Some(_) => Err("`@test` takes no arguments"),
            },
            _ => {
                out.push(BuiltinMatch {
                    range: d.range,
                    result: None,
                });
                continue;
            }
        };
        out.push(BuiltinMatch {
            range: d.range,
            result: Some(result),
        });
    }
    out
}
