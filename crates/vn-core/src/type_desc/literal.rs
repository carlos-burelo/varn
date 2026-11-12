use crate::TypeLiteral;
use std::fmt;

pub(super) fn encode(value: &TypeLiteral<String>, out: &mut String) {
    out.push('L');
    match value {
        TypeLiteral::Int(n) => {
            out.push('i');
            out.push_str(&n.to_string());
            out.push(';');
        }
        TypeLiteral::Str(s) => {
            out.push('s');
            out.push_str(&s.len().to_string());
            out.push(':');
            out.push_str(s);
        }
        TypeLiteral::Bool(b) => out.push(if *b { 't' } else { 'f' }),
        TypeLiteral::Char(c) => {
            out.push('c');
            out.push(*c);
        }
    }
}

pub(super) fn decode(rest: &mut &str) -> Option<TypeLiteral<String>> {
    let tag = rest.chars().next()?;
    *rest = &rest[tag.len_utf8()..];
    Some(match tag {
        'i' => TypeLiteral::Int(super::take_until(rest, ';')?.parse().ok()?),
        's' => {
            let len: usize = super::take_until(rest, ':')?.parse().ok()?;
            let value = rest.get(..len)?.to_owned();
            *rest = &rest[len..];
            TypeLiteral::Str(value)
        }
        't' => TypeLiteral::Bool(true),
        'f' => TypeLiteral::Bool(false),
        'c' => {
            let value = rest.chars().next()?;
            *rest = &rest[value.len_utf8()..];
            TypeLiteral::Char(value)
        }
        _ => return None,
    })
}

pub(super) fn display(value: &TypeLiteral<String>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match value {
        TypeLiteral::Int(n) => write!(f, "{n}"),
        TypeLiteral::Str(s) => write!(f, "{s:?}"),
        TypeLiteral::Bool(b) => write!(f, "{b}"),
        TypeLiteral::Char(c) => write!(f, "{c:?}"),
    }
}
