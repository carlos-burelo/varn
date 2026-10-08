use varn_core::ast::{AstArena, ExprId};
use varn_core::Atom;

pub(super) fn pattern_names(p: &varn_core::ast::Pattern) -> Vec<Atom> {
    use varn_core::ast::Pattern;
    let mut out = Vec::new();
    fn go(p: &Pattern, out: &mut Vec<Atom>) {
        match p {
            Pattern::Identifier { name, .. } => out.push(*name),
            Pattern::Array { elements, rest, .. } => {
                for e in elements.iter().flatten() {
                    go(&e.pattern, out);
                }
                if let Some(r) = rest {
                    go(r, out);
                }
            }
            Pattern::Object {
                properties, rest, ..
            } => {
                for pr in properties {
                    go(&pr.value, out);
                }
                if let Some(r) = rest {
                    go(r, out);
                }
            }
            Pattern::Assignment { left, .. } => go(left, out),
            Pattern::Rest { argument, .. } => go(argument, out),
        }
    }
    go(p, &mut out);
    out
}

pub(super) fn walk_expr_children(e: ExprId, arena: &AstArena, f: &mut dyn FnMut(ExprId)) {
    use varn_core::ast::{Arg, ArrayEl, ExprKind, MatchBody, ObjectProp, TemplatePart};
    match &arena.expr(e).kind {
        ExprKind::Template { parts } => {
            for p in parts {
                if let TemplatePart::Interpolation(x) = p {
                    f(*x);
                }
            }
        }
        ExprKind::TaggedTemplate { tag, template } => {
            f(*tag);
            f(*template);
        }
        ExprKind::Array { elements } => {
            for el in elements {
                match el {
                    ArrayEl::Expr(x) | ArrayEl::Spread(x) => f(*x),
                    ArrayEl::Hole => {}
                }
            }
        }
        ExprKind::Object { properties } | ExprKind::Record { properties } => {
            for p in properties {
                match p {
                    ObjectProp::Property { value, .. } => f(*value),
                    ObjectProp::Spread { argument, .. } => f(*argument),
                    ObjectProp::Method { .. }
                    | ObjectProp::Getter { .. }
                    | ObjectProp::Setter { .. } => {}
                }
            }
        }
        ExprKind::Tuple { elements } => elements.iter().for_each(|x| f(*x)),
        ExprKind::Unary { operand, .. }
        | ExprKind::Update { operand, .. }
        | ExprKind::Paren {
            expression: operand,
        }
        | ExprKind::Await { argument: operand }
        | ExprKind::Spawn { argument: operand }
        | ExprKind::NonNull {
            expression: operand,
        }
        | ExprKind::Try {
            expression: operand,
        }
        | ExprKind::As {
            expression: operand,
            ..
        }
        | ExprKind::Satisfies {
            expression: operand,
            ..
        }
        | ExprKind::Is {
            expression: operand,
            ..
        } => f(*operand),
        ExprKind::Yield { argument, .. } => {
            if let Some(x) = argument {
                f(*x);
            }
        }
        ExprKind::Binary { left, right, .. }
        | ExprKind::Logical { left, right, .. }
        | ExprKind::Pipeline { left, right }
        | ExprKind::Range {
            start: left,
            end: right,
            ..
        } => {
            f(*left);
            f(*right);
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            f(*test);
            f(*consequent);
            f(*alternate);
        }
        ExprKind::Member {
            object, property, ..
        } => {
            f(*object);
            f(*property);
        }
        ExprKind::Call { callee, args, .. } | ExprKind::New { callee, args, .. } => {
            f(*callee);
            for a in args {
                match a {
                    Arg::Positional(x) | Arg::Spread(x) | Arg::Named { value: x, .. } => f(*x),
                }
            }
        }
        ExprKind::Sequence { expressions } => expressions.iter().for_each(|x| f(*x)),
        ExprKind::MetaAccess { target, .. } => f(*target),
        ExprKind::With { object, properties } => {
            f(*object);
            for p in properties {
                if let ObjectProp::Property { value, .. } = p {
                    f(*value);
                }
            }
        }
        ExprKind::Match { subject, cases } => {
            f(*subject);
            for c in cases {
                if let Some(g) = c.guard {
                    f(g);
                }
                match &c.body {
                    MatchBody::Expr(x) => f(*x),
                    MatchBody::Block(_) => {}
                }
            }
        }
        ExprKind::IntLiteral { .. }
        | ExprKind::FloatLiteral { .. }
        | ExprKind::BigIntLiteral { .. }
        | ExprKind::DecimalLiteral { .. }
        | ExprKind::StrLiteral { .. }
        | ExprKind::CharLiteral { .. }
        | ExprKind::BoolLiteral { .. }
        | ExprKind::NullLiteral
        | ExprKind::RegexLiteral { .. }
        | ExprKind::Identifier { .. }
        | ExprKind::Missing
        | ExprKind::This
        | ExprKind::Super
        | ExprKind::Assign { .. }
        | ExprKind::Function { .. }
        | ExprKind::Arrow { .. }
        | ExprKind::Spread { .. }
        | ExprKind::ClassExpr { .. } => {}
    }
}
