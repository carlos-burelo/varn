use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeDesc {
    Any,
    Int,
    Float,
    Bool,
    Char,
    Str,
    Bytes,
    Decimal,
    BigInt,
    Callable,
    Array(Box<TypeDesc>),
    Map(Box<TypeDesc>, Box<TypeDesc>),
    Set(Box<TypeDesc>),
    Dict(Box<TypeDesc>),
    Tuple(Vec<TypeDesc>),
    Class {
        witness: u32,
        name: String,
    },
    Enum {
        witness: u32,
        name: String,
    },
    Nullable(Box<TypeDesc>),
    Args {
        fixed: Vec<TypeDesc>,
        rest: Option<Box<TypeDesc>>,
    },
    Object(Vec<FieldDesc>),
    Enclosing {
        up: u32,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDesc {
    pub name: String,
    pub ty: TypeDesc,
    pub optional: bool,
}

impl TypeDesc {
    pub fn is_any(&self) -> bool {
        matches!(self, TypeDesc::Any)
    }

    pub fn encode(&self) -> String {
        let mut out = String::new();
        self.encode_into(&mut out);
        out
    }

    fn encode_into(&self, out: &mut String) {
        match self {
            TypeDesc::Any => out.push('*'),
            TypeDesc::Int => out.push('i'),
            TypeDesc::Float => out.push('f'),
            TypeDesc::Bool => out.push('b'),
            TypeDesc::Char => out.push('c'),
            TypeDesc::Str => out.push('s'),
            TypeDesc::Bytes => out.push('y'),
            TypeDesc::Decimal => out.push('d'),
            TypeDesc::BigInt => out.push('g'),
            TypeDesc::Callable => out.push('F'),
            TypeDesc::Array(el) => {
                out.push('[');
                el.encode_into(out);
            }
            TypeDesc::Map(k, v) => {
                out.push('M');
                k.encode_into(out);
                v.encode_into(out);
            }
            TypeDesc::Set(el) => {
                out.push('Z');
                el.encode_into(out);
            }
            TypeDesc::Dict(el) => {
                out.push('D');
                el.encode_into(out);
            }
            TypeDesc::Tuple(els) => {
                out.push('T');
                out.push_str(&els.len().to_string());
                out.push(';');
                for el in els {
                    el.encode_into(out);
                }
            }
            TypeDesc::Class { witness, name } => {
                out.push('C');
                out.push_str(&witness.to_string());
                out.push(':');
                out.push_str(name);
                out.push(';');
            }
            TypeDesc::Enum { witness, name } => {
                out.push('E');
                out.push_str(&witness.to_string());
                out.push(':');
                out.push_str(name);
                out.push(';');
            }
            TypeDesc::Nullable(inner) => {
                out.push('?');
                inner.encode_into(out);
            }
            TypeDesc::Args { fixed, rest } => {
                out.push('A');
                out.push_str(&fixed.len().to_string());
                out.push(';');
                for el in fixed {
                    el.encode_into(out);
                }
                match rest {
                    Some(el) => {
                        out.push('+');
                        el.encode_into(out);
                    }
                    None => out.push('.'),
                }
            }
            TypeDesc::Enclosing { up, name } => {
                out.push('R');
                out.push_str(&up.to_string());
                out.push(':');
                out.push_str(name);
                out.push(';');
            }
            TypeDesc::Object(fields) => {
                out.push('O');
                out.push_str(&fields.len().to_string());
                out.push(';');
                for field in fields {
                    out.push_str(&field.name.len().to_string());
                    out.push(':');
                    out.push_str(&field.name);
                    out.push(if field.optional { '~' } else { '!' });
                    field.ty.encode_into(out);
                }
            }
        }
    }

    pub fn decode(text: &str) -> Option<TypeDesc> {
        let mut rest = text;
        let desc = decode_one(&mut rest)?;
        rest.is_empty().then_some(desc)
    }
}

fn take_until<'a>(rest: &mut &'a str, stop: char) -> Option<&'a str> {
    let at = rest.find(stop)?;
    let head = &rest[..at];
    *rest = &rest[at + stop.len_utf8()..];
    Some(head)
}

fn decode_one(rest: &mut &str) -> Option<TypeDesc> {
    let tag = rest.chars().next()?;
    *rest = &rest[tag.len_utf8()..];
    Some(match tag {
        '*' => TypeDesc::Any,
        'i' => TypeDesc::Int,
        'f' => TypeDesc::Float,
        'b' => TypeDesc::Bool,
        'c' => TypeDesc::Char,
        's' => TypeDesc::Str,
        'y' => TypeDesc::Bytes,
        'd' => TypeDesc::Decimal,
        'g' => TypeDesc::BigInt,
        'F' => TypeDesc::Callable,
        '[' => TypeDesc::Array(Box::new(decode_one(rest)?)),
        'M' => {
            let k = decode_one(rest)?;
            let v = decode_one(rest)?;
            TypeDesc::Map(Box::new(k), Box::new(v))
        }
        'Z' => TypeDesc::Set(Box::new(decode_one(rest)?)),
        'D' => TypeDesc::Dict(Box::new(decode_one(rest)?)),
        'T' => {
            let count: usize = take_until(rest, ';')?.parse().ok()?;
            let mut els = Vec::with_capacity(count);
            for _ in 0..count {
                els.push(decode_one(rest)?);
            }
            TypeDesc::Tuple(els)
        }
        'C' => {
            let witness = take_until(rest, ':')?.parse().ok()?;
            let name = take_until(rest, ';')?.to_string();
            TypeDesc::Class { witness, name }
        }
        'E' => {
            let witness = take_until(rest, ':')?.parse().ok()?;
            let name = take_until(rest, ';')?.to_string();
            TypeDesc::Enum { witness, name }
        }
        '?' => TypeDesc::Nullable(Box::new(decode_one(rest)?)),
        'A' => {
            let count: usize = take_until(rest, ';')?.parse().ok()?;
            let mut fixed = Vec::with_capacity(count);
            for _ in 0..count {
                fixed.push(decode_one(rest)?);
            }
            let marker = rest.chars().next()?;
            *rest = &rest[marker.len_utf8()..];
            let tail = match marker {
                '+' => Some(Box::new(decode_one(rest)?)),
                '.' => None,
                _ => return None,
            };
            TypeDesc::Args { fixed, rest: tail }
        }
        'R' => {
            let up = take_until(rest, ':')?.parse().ok()?;
            let name = take_until(rest, ';')?.to_string();
            TypeDesc::Enclosing { up, name }
        }
        'O' => {
            let count: usize = take_until(rest, ';')?.parse().ok()?;
            let mut fields = Vec::with_capacity(count);
            for _ in 0..count {
                let len: usize = take_until(rest, ':')?.parse().ok()?;
                let name = rest.get(..len)?.to_string();
                *rest = &rest[len..];
                let marker = rest.chars().next()?;
                *rest = &rest[marker.len_utf8()..];
                let optional = match marker {
                    '~' => true,
                    '!' => false,
                    _ => return None,
                };
                fields.push(FieldDesc {
                    name,
                    ty: decode_one(rest)?,
                    optional,
                });
            }
            TypeDesc::Object(fields)
        }
        _ => return None,
    })
}

impl fmt::Display for TypeDesc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeDesc::Any => f.write_str("dynamic"),
            TypeDesc::Int => f.write_str("int"),
            TypeDesc::Float => f.write_str("float"),
            TypeDesc::Bool => f.write_str("bool"),
            TypeDesc::Char => f.write_str("char"),
            TypeDesc::Str => f.write_str("str"),
            TypeDesc::Bytes => f.write_str("bytes"),
            TypeDesc::Decimal => f.write_str("decimal"),
            TypeDesc::BigInt => f.write_str("bigint"),
            TypeDesc::Callable => f.write_str("function"),
            TypeDesc::Array(el) => write!(f, "{el}[]"),
            TypeDesc::Map(k, v) => write!(f, "Map<{k}, {v}>"),
            TypeDesc::Set(el) => write!(f, "Set<{el}>"),
            TypeDesc::Dict(el) => write!(f, "{{ [key: str]: {el} }}"),
            TypeDesc::Tuple(els) => {
                f.write_str("#[")?;
                for (i, el) in els.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{el}")?;
                }
                f.write_str("]")
            }
            TypeDesc::Class { name, .. } => f.write_str(name),
            TypeDesc::Enum { name, .. } => f.write_str(name),
            TypeDesc::Nullable(inner) => write!(f, "{inner}?"),
            TypeDesc::Args { fixed, rest } => {
                f.write_str("(")?;
                for (i, el) in fixed.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{el}")?;
                }
                if let Some(el) = rest {
                    if !fixed.is_empty() {
                        f.write_str(", ")?;
                    }
                    write!(f, "...{el}")?;
                }
                f.write_str(")")
            }
            TypeDesc::Enclosing { name, .. } => f.write_str(name),
            TypeDesc::Object(fields) => {
                f.write_str("{ ")?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    if let (true, TypeDesc::Nullable(present)) = (field.optional, &field.ty) {
                        write!(f, "{}?: {present}", field.name)?;
                    } else {
                        let mark = if field.optional { "?" } else { "" };
                        write!(f, "{}{mark}: {}", field.name, field.ty)?;
                    }
                }
                f.write_str(" }")
            }
        }
    }
}
