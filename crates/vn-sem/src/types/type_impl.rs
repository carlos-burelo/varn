use super::*;
use std::sync::Arc;

#[allow(non_upper_case_globals)]
impl Type {
    pub const Int: Type = Type::resolved(CheckerTyId::INT);
    pub const Float: Type = Type::resolved(CheckerTyId::FLOAT);
    pub const Decimal: Type = Type::resolved(CheckerTyId::DECIMAL);
    pub const BigInt: Type = Type::resolved(CheckerTyId::BIGINT);
    pub const Str: Type = Type::resolved(CheckerTyId::STR);
    pub const Char: Type = Type::resolved(CheckerTyId::CHAR);
    pub const Bool: Type = Type::resolved(CheckerTyId::BOOL);
    pub const Void: Type = Type::resolved(CheckerTyId::VOID);
    pub const Null: Type = Type::resolved(CheckerTyId::NULL);
    pub const Never: Type = Type::resolved(CheckerTyId::NEVER);
    pub const Dynamic: Type = Type::resolved(CheckerTyId::DYNAMIC);
    pub const Error: Type = Type::resolved(CheckerTyId::ERROR);
    pub const This: Type = Type::resolved(CheckerTyId::THIS);

    pub fn primitive(p: vn_core::LangPrimitive, table: &mut CheckerTyTable) -> Self {
        Type::resolved(table.intern(TypeKind::Primitive(p)))
    }

    pub fn apparent(&self, table: &CheckerTyTable) -> Type {
        fn base(p: vn_core::LangPrimitive) -> Type {
            use vn_core::LangPrimitive as P;
            match p {
                P::Int => Type::Int,
                P::Str => Type::Str,
                P::Bool => Type::Bool,
                P::Char => Type::Char,
                P::Error => Type::Error,
                P::Null | P::Float | P::BigInt | P::Decimal | P::Void | P::Never => Type::Dynamic,
                P::Dynamic => Type::Dynamic,
            }
        }
        match table.get(self.0) {
            TypeKind::Literal(l) => base(l.base()),
            TypeKind::Union(list) => {
                let mut shared = None;
                for m in table.get_list(list) {
                    let TypeKind::Literal(l) = table.get(*m) else {
                        return *self;
                    };
                    match shared {
                        None => shared = Some(l.base()),
                        Some(b) if b == l.base() => {}
                        Some(_) => return *self,
                    }
                }
                shared.map_or(*self, base)
            }
            TypeKind::Primitive(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Record(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => *self,
        }
    }

    pub fn range_over(bound: &Type, table: &mut CheckerTyTable) -> Self {
        let elem = if bound.apparent(table) == Type::Char {
            Type::Char
        } else {
            Type::Int
        };
        Type::builtin(vn_core::BuiltinType::Range, vec![elem], table)
    }

    pub fn is_range(&self, table: &CheckerTyTable) -> bool {
        self.is_builtin(vn_core::BuiltinType::Range, table)
    }

    pub fn literal(l: vn_core::TypeLiteral<vn_core::Atom>, table: &mut CheckerTyTable) -> Self {
        Type::resolved(table.intern(TypeKind::Literal(l)))
    }

    pub fn sanitize_foreign(self) -> Type {
        self
    }

    pub fn get_array_element_type(&self, table: &CheckerTyTable) -> Type {
        self.sequence_element(table).unwrap_or(Type::Dynamic)
    }

    pub fn fn_(f: FunctionType, table: &mut CheckerTyTable) -> Self {
        let fid = table.intern_function(f);
        Type::resolved(table.intern(TypeKind::Fn(fid)))
    }

    pub fn named(name: impl Into<Arc<str>>, table: &mut CheckerTyTable) -> Self {
        let atom = table.intern_name(&name.into());
        Type::resolved(table.intern(TypeKind::Named(atom, None)))
    }

    pub fn named_atom(name: vn_core::Atom, table: &mut CheckerTyTable) -> Self {
        Type::resolved(table.intern(TypeKind::Named(name, None)))
    }

    pub fn named_with_origin_atom(
        name: vn_core::Atom,
        origin: Option<vn_core::Atom>,
        table: &mut CheckerTyTable,
    ) -> Self {
        Type::resolved(table.intern(TypeKind::Named(name, origin)))
    }

    pub fn named_with_origin(
        name: impl Into<Arc<str>>,
        origin: Option<Arc<str>>,
        table: &mut CheckerTyTable,
    ) -> Self {
        let name_atom = table.intern_name(&name.into());
        let origin_atom = origin.map(|o| table.intern_name(&o));
        Type::named_with_origin_atom(name_atom, origin_atom, table)
    }

    pub fn generic(name: impl Into<Arc<str>>, args: Vec<Type>, table: &mut CheckerTyTable) -> Self {
        let atom = table.intern_name(&name.into());
        Type::generic_atom(atom, args, None, table)
    }

    pub fn generic_with_origin(
        name: impl Into<Arc<str>>,
        args: Vec<Type>,
        origin: Option<Arc<str>>,
        table: &mut CheckerTyTable,
    ) -> Self {
        let atom = table.intern_name(&name.into());
        let origin_atom = origin.map(|o| table.intern_name(&o));
        Type::generic_atom(atom, args, origin_atom, table)
    }

    pub fn array(inner: Type, table: &mut CheckerTyTable) -> Self {
        Type::resolved(table.intern(TypeKind::Array(inner.0)))
    }

    pub fn generic_atom(
        name: vn_core::Atom,
        args: Vec<Type>,
        origin: Option<vn_core::Atom>,
        table: &mut CheckerTyTable,
    ) -> Self {
        let ids: Vec<CheckerTyId> = args.iter().map(|a| a.0).collect();
        let list = table.intern_list(&ids);
        Type::resolved(table.intern(TypeKind::Generic(name, list, origin)))
    }

    pub fn record(members: Vec<ObjectTypeMember>, table: &mut CheckerTyTable) -> Self {
        let mid = table.intern_object_members(members);
        Type::resolved(table.intern(TypeKind::Record(mid)))
    }

    pub fn interface(members: Vec<ObjectTypeMember>, table: &mut CheckerTyTable) -> Self {
        let mid = table.intern_object_members(members);
        Type::resolved(table.intern(TypeKind::Interface(mid)))
    }

    pub fn union(members: Vec<Type>, table: &mut CheckerTyTable) -> Self {
        if members.iter().any(|m| m.is_error()) {
            return Type::Error;
        }
        let had_members = !members.is_empty();
        let members: Vec<Type> = members
            .into_iter()
            .filter(|m| m.0 != CheckerTyId::NEVER)
            .collect();
        if had_members && members.is_empty() {
            return Type::Never;
        }
        if members.len() == 1 {
            if !matches!(table.get(members[0].0), TypeKind::Union(_)) {
                return members.into_iter().next().unwrap();
            }
        } else if members.len() == 2 {
            if !matches!(table.get(members[0].0), TypeKind::Union(_))
                && !matches!(table.get(members[1].0), TypeKind::Union(_))
            {
                if members[0] == members[1] {
                    return members.into_iter().next().unwrap();
                } else {
                    let ids: Vec<CheckerTyId> = members.iter().map(|m| m.0).collect();
                    let list = table.intern_list(&ids);
                    return Type::resolved(table.intern(TypeKind::Union(list)));
                }
            }
        } else if members.is_empty() {
            let list = table.intern_list(&[]);
            return Type::resolved(table.intern(TypeKind::Union(list)));
        }

        let mut seen = rustc_hash::FxHashSet::default();
        let mut flat: Vec<Type> = Vec::with_capacity(members.len());
        for m in members {
            match table.get(m.0) {
                TypeKind::Union(inner_list) => {
                    for id in table.get_list(inner_list).to_vec() {
                        let t = Type::resolved(id);
                        if t.0 != CheckerTyId::NEVER && seen.insert(t) {
                            flat.push(t);
                        }
                    }
                }
                TypeKind::Primitive(_)
                | TypeKind::Literal(_)
                | TypeKind::This
                | TypeKind::Array(_)
                | TypeKind::Intersection(_)
                | TypeKind::Tuple(_)
                | TypeKind::Named(..)
                | TypeKind::Generic(..)
                | TypeKind::TemplateLiteral(_)
                | TypeKind::Fn(_)
                | TypeKind::Record(_)
                | TypeKind::Interface(_)
                | TypeKind::Typeof(_)
                | TypeKind::KeyOf(_)
                | TypeKind::IndexedAccess { .. }
                | TypeKind::Mapped { .. }
                | TypeKind::Conditional { .. }
                | TypeKind::Infer(_)
                | TypeKind::EnumVariant { .. }
                | TypeKind::TypePredicate { .. } => {
                    if seen.insert(m) {
                        flat.push(m);
                    }
                }
            }
        }
        if flat.len() == 1 {
            flat.remove(0)
        } else {
            let ids: Vec<CheckerTyId> = flat.iter().map(|m| m.0).collect();
            let list = table.intern_list(&ids);
            Type::resolved(table.intern(TypeKind::Union(list)))
        }
    }

    pub fn is_dynamic(&self) -> bool {
        self.0 == CheckerTyId::DYNAMIC
    }

    pub fn is_unresolved(&self) -> bool {
        self.is_dynamic() || self.is_error()
    }

    pub fn is_int(&self) -> bool {
        self.0 == CheckerTyId::INT
    }
    pub fn is_float(&self) -> bool {
        self.0 == CheckerTyId::FLOAT
    }
    pub fn is_numeric(&self) -> bool {
        self.is_int()
            || self.is_float()
            || matches!(self.0, CheckerTyId::DECIMAL | CheckerTyId::BIGINT)
    }
    pub fn is_str(&self) -> bool {
        self.0 == CheckerTyId::STR
    }
    pub fn is_bool(&self) -> bool {
        self.0 == CheckerTyId::BOOL
    }
    pub fn is_void(&self) -> bool {
        self.0 == CheckerTyId::VOID
    }
    pub fn is_never(&self) -> bool {
        self.0 == CheckerTyId::NEVER
    }

    pub fn stdlib_key<'t>(&self, table: &'t CheckerTyTable) -> Option<&'t str> {
        match table.get(self.0) {
            TypeKind::Primitive(p) => match p {
                vn_core::LangPrimitive::Int => Some(vn_core::LangPrimitive::Int.name()),
                vn_core::LangPrimitive::Float => Some(vn_core::LangPrimitive::Float.name()),
                vn_core::LangPrimitive::Decimal => Some(vn_core::LangPrimitive::Decimal.name()),
                vn_core::LangPrimitive::BigInt => Some(vn_core::LangPrimitive::BigInt.name()),
                vn_core::LangPrimitive::Str => Some(vn_core::LangPrimitive::Str.name()),
                vn_core::LangPrimitive::Char => Some(vn_core::LangPrimitive::Char.name()),
                vn_core::LangPrimitive::Bool => Some(vn_core::LangPrimitive::Bool.name()),
                vn_core::LangPrimitive::Null
                | vn_core::LangPrimitive::Void
                | vn_core::LangPrimitive::Never
                | vn_core::LangPrimitive::Dynamic
                | vn_core::LangPrimitive::Error => None,
            },
            TypeKind::Array(_) => Some(vn_core::BuiltinType::Array.name()),
            TypeKind::Named(..) if self.is_bytes(table) => {
                Some(vn_core::BuiltinType::Bytes.name())
            }
            TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Record(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => None,
        }
    }

    pub fn descriptor_key<'t>(
        &self,
        table: &'t CheckerTyTable,
        interner: &'t vn_core::AtomInterner,
    ) -> Option<&'t str> {
        if let Some(k) = self.stdlib_key(table) {
            return Some(k);
        }
        match table.get(self.0) {
            TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => Some(interner.resolve(n)),
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Record(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => None,
        }
    }
}
