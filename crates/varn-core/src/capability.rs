//! Operators a user type answers through a capability method (spec §33–§34).
//! One table: the checker reads it to type the operator, the emitter to
//! lower it to the method call.

use crate::ast::operators::{BinaryOp, UnaryOp};

/// How the method's result becomes the operator's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperatorShape {
    /// `a + b` is `a.add(b)`.
    Value,
    /// `a < b` is `a.compare(b) < 0` (same comparison against zero).
    CompareToZero(BinaryOp),
    /// `a == b` is `a.equals(b)`.
    Equals,
    /// `a != b` is `!a.equals(b)`.
    NotEquals,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperatorMethod {
    pub capability: &'static str,
    pub method: &'static str,
    pub shape: OperatorShape,
}

const fn value(capability: &'static str, method: &'static str) -> OperatorMethod {
    OperatorMethod {
        capability,
        method,
        shape: OperatorShape::Value,
    }
}

pub const fn binary_operator_method(op: BinaryOp) -> Option<OperatorMethod> {
    use BinaryOp as B;
    Some(match op {
        B::Add => value("Add", "add"),
        B::Sub => value("Sub", "sub"),
        B::Mul => value("Mul", "mul"),
        B::Div => value("Div", "div"),
        B::Lt | B::Gt | B::LtEq | B::GtEq => OperatorMethod {
            capability: "Comparable",
            method: "compare",
            shape: OperatorShape::CompareToZero(op),
        },
        B::Eq => OperatorMethod {
            capability: "Equatable",
            method: "equals",
            shape: OperatorShape::Equals,
        },
        B::NotEq => OperatorMethod {
            capability: "Equatable",
            method: "equals",
            shape: OperatorShape::NotEquals,
        },
        B::Mod
        | B::Pow
        | B::BitAnd
        | B::BitOr
        | B::BitXor
        | B::Shl
        | B::Shr
        | B::UShr
        | B::Instanceof
        | B::In => return None,
    })
}

pub const fn unary_operator_method(op: UnaryOp) -> Option<OperatorMethod> {
    match op {
        UnaryOp::Minus => Some(value("Neg", "neg")),
        UnaryOp::Plus | UnaryOp::Not | UnaryOp::BitNot | UnaryOp::Typeof => None,
    }
}
