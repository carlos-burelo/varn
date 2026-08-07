use super::*;

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            TypeKind::Intrinsic(tag) => write!(f, "{}", tag.name()),
            TypeKind::This => write!(f, "this"),
            TypeKind::Array(t) => match &t.0 {
                TypeKind::Union(_)
                | TypeKind::Intersection(_)
                | TypeKind::Fn(_)
                | TypeKind::Conditional { .. } => write!(f, "({t})[]"),
                _ => write!(f, "{t}[]"),
            },
            TypeKind::Union(members) => {
                let non_null: Vec<_> = members.iter().filter(|m| !m.is_nullable()).collect();
                if non_null.len() == 1 && non_null.len() < members.len() {
                    return write!(f, "{}?", non_null[0]);
                }
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{m}")?;
                }
                Ok(())
            }
            TypeKind::Named(name, origin) => {
                if name.as_ref() == "*" {
                    if let Some(orig) = origin {
                        write!(f, "namespace {orig}")
                    } else {
                        write!(f, "namespace")
                    }
                } else {
                    write!(f, "{name}")
                }
            }
            TypeKind::Generic(name, args, _origin) => {
                write!(f, "{name}<")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                write!(f, ">")
            }
            TypeKind::TemplateLiteral(parts) => {
                write!(f, "`")?;
                for (i, part) in parts.iter().enumerate() {
                    if i % 2 == 0 {
                        write!(f, "{part}")?;
                    } else {
                        write!(f, "${{{part}}}")?;
                    }
                }
                write!(f, "`")
            }
            TypeKind::Fn(ft) => {
                write!(f, "(")?;
                for (i, p) in ft.params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    if let Some(name) = &p.name {
                        let prefix = if p.is_rest { "..." } else { "" };
                        write!(f, "{prefix}{name}: ")?;
                    } else if p.is_rest {
                        write!(f, "...")?;
                    }
                    write!(f, "{}", p.ty)?;
                    if p.optional {
                        write!(f, "?")?;
                    }
                }
                write!(f, ") => {}", ft.return_type)
            }
            TypeKind::Object(members) => {
                write!(f, "#{{ ")?;
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    match m {
                        ObjectTypeMember::Property {
                            name,
                            ty,
                            optional,
                            readonly,
                        } => {
                            if *readonly {
                                write!(f, "readonly ")?;
                            }
                            write!(f, "{}{}: {}", name, if *optional { "?" } else { "" }, ty)?;
                        }
                        ObjectTypeMember::Method {
                            name,
                            params,
                            return_type,
                            optional,
                            is_arrow: _,
                        } => {
                            write!(f, "{}{}(", name, if *optional { "?" } else { "" })?;
                            for (j, p) in params.iter().enumerate() {
                                if j > 0 {
                                    write!(f, ", ")?;
                                }
                                if let Some(pname) = &p.name {
                                    write!(f, "{pname}: ")?;
                                }
                                write!(f, "{}", p.ty)?;
                                if p.optional {
                                    write!(f, "?")?;
                                }
                            }
                            write!(f, "): {return_type}")?;
                        }
                        ObjectTypeMember::Index {
                            param_name,
                            key_ty,
                            value_ty,
                        } => {
                            write!(f, "[{param_name}: {key_ty}]: {value_ty}")?;
                        }
                        ObjectTypeMember::Callable {
                            params,
                            return_type,
                            is_arrow: _,
                        } => {
                            write!(f, "(")?;
                            for (j, p) in params.iter().enumerate() {
                                if j > 0 {
                                    write!(f, ", ")?;
                                }
                                write!(f, "{}", p.ty)?;
                            }
                            write!(f, "): {return_type}")?;
                        }
                    }
                }
                write!(f, " }}")
            }
            TypeKind::Intersection(members) => {
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        write!(f, " & ")?;
                    }
                    match &m.0 {
                        TypeKind::Union(_) => write!(f, "({m})")?,
                        _ => write!(f, "{m}")?,
                    }
                }
                Ok(())
            }
            TypeKind::Tuple(members) => {
                write!(f, "#[")?;
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{m}")?;
                }
                write!(f, "]")
            }
            TypeKind::Typeof(_) => write!(f, "typeof <expr>"),
            TypeKind::EnumVariant { .. } => write!(f, "enum variant"),

            TypeKind::KeyOf(t) => write!(f, "keyof {t}"),
            TypeKind::IndexedAccess { object, index } => write!(f, "{object}[{index}]"),
            TypeKind::Mapped {
                key_var,
                source,
                value,
                optional,
                readonly,
            } => {
                write!(
                    f,
                    "{{ {}[{} in {}]{}: {} }}",
                    if *readonly { "readonly " } else { "" },
                    key_var,
                    source,
                    if *optional { "?" } else { "" },
                    value
                )
            }
            TypeKind::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                write!(f, "{check} extends {extends} ? {true_type} : {false_type}")
            }
            TypeKind::Infer(name) => write!(f, "infer {name}"),
        }
    }
}
