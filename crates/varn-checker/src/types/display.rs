use super::*;

/// Renders a `Type` to text. `Type` itself no longer implements
/// `fmt::Display` — same reasoning as `varn_core::Atom`, which never has: a
/// bare `CheckerTyId` (or, inside a `Named`/`Generic`, a bare `Atom`) cannot
/// print itself without the table/interner that gave it meaning. Every call
/// site that used to write `{ty}`/`ty.to_string()` now writes
/// `ty.display(table, interner)` (or, in contexts with a combined view,
/// whatever convenience wrapper that call site's task adds around this).
pub struct TypeDisplay<'t> {
    ty: Type,
    table: &'t CheckerTyTable,
    interner: &'t varn_core::AtomInterner,
}

impl Type {
    pub fn display<'t>(
        &self,
        table: &'t CheckerTyTable,
        interner: &'t varn_core::AtomInterner,
    ) -> TypeDisplay<'t> {
        TypeDisplay {
            ty: *self,
            table,
            interner,
        }
    }
}

impl<'t> TypeDisplay<'t> {
    fn child(&self, id: CheckerTyId) -> TypeDisplay<'t> {
        TypeDisplay {
            ty: Type(id, false),
            table: self.table,
            interner: self.interner,
        }
    }

    fn name(&self, atom: varn_core::Atom) -> &'t str {
        self.interner.resolve(atom)
    }
}

impl fmt::Display for TypeDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let table = self.table;
        match table.get(self.ty.0) {
            TypeKind::Intrinsic(tag) => write!(f, "{}", tag.name()),
            TypeKind::This => write!(f, "this"),
            TypeKind::Array(t) => {
                let t = *t;
                match table.get(t) {
                    TypeKind::Union(_)
                    | TypeKind::Intersection(_)
                    | TypeKind::Fn(_)
                    | TypeKind::Conditional { .. } => write!(f, "({})[]", self.child(t)),
                    _ => write!(f, "{}[]", self.child(t)),
                }
            }
            TypeKind::Union(list) => {
                let members: Vec<CheckerTyId> = table.get_list(*list).to_vec();
                let non_null: Vec<CheckerTyId> = members
                    .iter()
                    .filter(|m| !Type(**m, false).is_nullable(table))
                    .copied()
                    .collect();
                if non_null.len() == 1 && non_null.len() < members.len() {
                    return write!(f, "{}?", self.child(non_null[0]));
                }
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{}", self.child(*m))?;
                }
                Ok(())
            }
            TypeKind::Named(name, origin) => {
                let name_str = self.name(*name);
                if name_str == "*" {
                    if let Some(orig) = origin {
                        write!(f, "namespace {}", self.name(*orig))
                    } else {
                        write!(f, "namespace")
                    }
                } else {
                    write!(f, "{name_str}")
                }
            }
            TypeKind::Generic(name, args, _origin) => {
                write!(f, "{}<", self.name(*name))?;
                for (i, arg) in table.get_list(*args).iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", self.child(*arg))?;
                }
                write!(f, ">")
            }
            TypeKind::TemplateLiteral(parts) => {
                write!(f, "`")?;
                for (i, part) in table.get_list(*parts).iter().enumerate() {
                    if i % 2 == 0 {
                        write!(f, "{}", self.child(*part))?;
                    } else {
                        write!(f, "${{{}}}", self.child(*part))?;
                    }
                }
                write!(f, "`")
            }
            TypeKind::Fn(fid) => {
                let ft = table.get_function(*fid);
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
                    write!(f, "{}", self.child(p.ty))?;
                    if p.optional {
                        write!(f, "?")?;
                    }
                }
                write!(f, ") => {}", self.child(ft.return_type))
            }
            TypeKind::Object(mid) => {
                write!(f, "{{ ")?;
                for (i, m) in table.get_object_members(*mid).iter().enumerate() {
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
                            write!(
                                f,
                                "{}{}: {}",
                                name,
                                if *optional { "?" } else { "" },
                                self.child(*ty)
                            )?;
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
                                write!(f, "{}", self.child(p.ty))?;
                                if p.optional {
                                    write!(f, "?")?;
                                }
                            }
                            write!(f, "): {}", self.child(*return_type))?;
                        }
                        ObjectTypeMember::Index {
                            param_name,
                            key_ty,
                            value_ty,
                        } => {
                            write!(
                                f,
                                "[{param_name}: {}]: {}",
                                self.child(*key_ty),
                                self.child(*value_ty)
                            )?;
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
                                write!(f, "{}", self.child(p.ty))?;
                            }
                            write!(f, "): {}", self.child(*return_type))?;
                        }
                    }
                }
                write!(f, " }}")
            }
            TypeKind::Intersection(list) => {
                for (i, m) in table.get_list(*list).iter().enumerate() {
                    if i > 0 {
                        write!(f, " & ")?;
                    }
                    match table.get(*m) {
                        TypeKind::Union(_) => write!(f, "({})", self.child(*m))?,
                        _ => write!(f, "{}", self.child(*m))?,
                    }
                }
                Ok(())
            }
            TypeKind::Tuple(list) => {
                write!(f, "#[")?;
                for (i, m) in table.get_list(*list).iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", self.child(*m))?;
                }
                write!(f, "]")
            }
            TypeKind::Typeof(_) => write!(f, "typeof <expr>"),
            TypeKind::EnumVariant { .. } => write!(f, "enum variant"),

            TypeKind::KeyOf(t) => write!(f, "keyof {}", self.child(*t)),
            TypeKind::IndexedAccess { object, index } => {
                write!(f, "{}[{}]", self.child(*object), self.child(*index))
            }
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
                    self.name(*key_var),
                    self.child(*source),
                    if *optional { "?" } else { "" },
                    self.child(*value)
                )
            }
            TypeKind::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                write!(
                    f,
                    "{} extends {} ? {} : {}",
                    self.child(*check),
                    self.child(*extends),
                    self.child(*true_type),
                    self.child(*false_type)
                )
            }
            TypeKind::Infer(name) => write!(f, "infer {}", self.name(*name)),
            TypeKind::TypePredicate {
                parameter_name,
                target_type,
            } => {
                write!(
                    f,
                    "{} is {}",
                    self.name(*parameter_name),
                    self.child(*target_type)
                )
            }
        }
    }
}
