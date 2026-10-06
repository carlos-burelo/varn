use super::index::SpatialEntry;
use super::stmts::{collect_class_decl, collect_stmt};
use varn_core::ast::{
    Arg, ArrayEl, ArrowBody, AstArena, ExprId, ExprKind, MatchBody, MatchCase, ObjectProp, PropKey,
    TemplatePart,
};

pub(super) fn collect_expr(a: &AstArena, id: &ExprId, out: &mut Vec<SpatialEntry>) {
    let expr = a.expr(*id);
    out.push(SpatialEntry {
        start: expr.range.start.offset,
        end: expr.range.end.offset,
        expr: *id,
    });

    match &expr.kind {
        ExprKind::TaggedTemplate { tag, template } => {
            collect_expr(a, tag, out);
            collect_expr(a, template, out);
        }
        ExprKind::Template { parts } => {
            for part in parts {
                if let TemplatePart::Interpolation(e) = part {
                    collect_expr(a, e, out);
                }
            }
        }
        ExprKind::Array { elements } => {
            for el in elements {
                match el {
                    ArrayEl::Expr(e) | ArrayEl::Spread(e) => collect_expr(a, e, out),
                    ArrayEl::Hole => {}
                }
            }
        }
        ExprKind::Object { properties } | ExprKind::Record { properties } => {
            for prop in properties {
                collect_object_prop(a, prop, out);
            }
        }
        ExprKind::Tuple { elements }
        | ExprKind::Sequence {
            expressions: elements,
        } => {
            for el in elements {
                collect_expr(a, el, out);
            }
        }
        ExprKind::Unary { operand, .. }
        | ExprKind::Update { operand, .. }
        | ExprKind::Paren {
            expression: operand,
        }
        | ExprKind::Await { argument: operand }
        | ExprKind::Spawn { argument: operand }
        | ExprKind::Spread { argument: operand }
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
        } => {
            collect_expr(a, operand, out);
        }
        ExprKind::Binary { left, right, .. }
        | ExprKind::Logical { left, right, .. }
        | ExprKind::Assign {
            target: left,
            value: right,
            ..
        }
        | ExprKind::Pipeline { left, right }
        | ExprKind::Range {
            start: left,
            end: right,
            ..
        } => {
            collect_expr(a, left, out);
            collect_expr(a, right, out);
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_expr(a, test, out);
            collect_expr(a, consequent, out);
            collect_expr(a, alternate, out);
        }
        ExprKind::Member {
            object, property, ..
        } => {
            collect_expr(a, object, out);
            collect_expr(a, property, out);
        }
        ExprKind::Call { callee, args, .. } | ExprKind::New { callee, args, .. } => {
            collect_expr(a, callee, out);
            for arg in args {
                match arg {
                    Arg::Positional(e) | Arg::Spread(e) | Arg::Named { value: e, .. } => {
                        collect_expr(a, e, out);
                    }
                }
            }
        }
        ExprKind::Function { body, params, .. } => {
            for p in params {
                if let Some(default) = &p.default {
                    collect_expr(a, default, out);
                }
            }
            collect_stmt(a, body, out);
        }
        ExprKind::Arrow { params, body, .. } => {
            for p in params {
                if let Some(default) = &p.default {
                    collect_expr(a, default, out);
                }
            }
            match body.as_ref() {
                ArrowBody::Expr(e) => collect_expr(a, e, out),
                ArrowBody::Block(s) => collect_stmt(a, s, out),
            }
        }
        ExprKind::Yield {
            argument: Some(arg),
            ..
        } => {
            collect_expr(a, arg, out);
        }
        ExprKind::ClassExpr { declaration } => {
            collect_class_decl(a, declaration, out);
        }
        ExprKind::Match { subject, cases } => {
            collect_expr(a, subject, out);
            for c in cases {
                collect_match_case(a, c, out);
            }
        }
        ExprKind::With { object, properties } => {
            collect_expr(a, object, out);
            for p in properties {
                collect_object_prop(a, p, out);
            }
        }
        ExprKind::MetaAccess { target, .. } => {
            collect_expr(a, target, out);
        }
        _ => {}
    }
}

pub(super) fn collect_object_prop(a: &AstArena, prop: &ObjectProp, out: &mut Vec<SpatialEntry>) {
    match prop {
        ObjectProp::Property { key, value, .. } => {
            if let PropKey::Computed(e) = key {
                collect_expr(a, e, out);
            }
            collect_expr(a, value, out);
        }
        ObjectProp::Method {
            key, params, body, ..
        } => {
            if let PropKey::Computed(e) = key {
                collect_expr(a, e, out);
            }
            for p in params {
                if let Some(default) = &p.default {
                    collect_expr(a, default, out);
                }
            }
            collect_stmt(a, body, out);
        }
        ObjectProp::Getter { key, body, .. } => {
            if let PropKey::Computed(e) = key {
                collect_expr(a, e, out);
            }
            collect_stmt(a, body, out);
        }
        ObjectProp::Setter {
            key, param, body, ..
        } => {
            if let PropKey::Computed(e) = key {
                collect_expr(a, e, out);
            }
            if let Some(default) = &param.default {
                collect_expr(a, default, out);
            }
            collect_stmt(a, body, out);
        }
        ObjectProp::Spread { argument, .. } => {
            collect_expr(a, argument, out);
        }
    }
}

pub(super) fn collect_match_case(a: &AstArena, case: &MatchCase, out: &mut Vec<SpatialEntry>) {
    if let Some(guard) = &case.guard {
        collect_expr(a, guard, out);
    }
    match &case.body {
        MatchBody::Expr(e) => collect_expr(a, e, out),
        MatchBody::Block(s) => collect_stmt(a, s, out),
    }
}
