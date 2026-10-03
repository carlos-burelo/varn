use super::*;
use std::sync::Arc;

#[allow(non_upper_case_globals)]
impl Type {
    // ── Intrinsic constants ──────────────────────────────────────────────
    //
    // `CheckerTyTable::new` pre-seeds every one of these shapes at a FIXED
    // `CheckerTyId` (see `interned.rs`), so these stay `const` even though
    // `Type` now wraps a hash-consed id instead of an owned tree: no table
    // access is needed to produce them, only to look one up later.
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
    pub const Error: Type = Type(CheckerTyId::DYNAMIC, Origin::Error);
    pub const This: Type = Type::resolved(CheckerTyId::THIS);

    /// Primitives have fixed ids (the constants above); interning goes through
    /// the table so every spelling of one lands on the same id.
    pub fn primitive(p: varn_core::LangPrimitive, table: &mut CheckerTyTable) -> Self {
        Type::resolved(table.intern(TypeKind::Primitive(p)))
    }

    /// The type operators and member lookup see: a literal type (or a union
    /// of literals sharing one base) behaves as its base primitive.
    pub fn apparent(&self, table: &CheckerTyTable) -> Type {
        fn base(p: varn_core::LangPrimitive) -> Type {
            use varn_core::LangPrimitive as P;
            match p {
                P::Int => Type::Int,
                P::Str => Type::Str,
                P::Bool => Type::Bool,
                P::Char => Type::Char,
                P::Null | P::Float | P::BigInt | P::Decimal | P::Void | P::Never | P::Dynamic => {
                    Type::Dynamic
                }
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
            _ => *self,
        }
    }

    /// `Range<T>` over the domain of a range bound: `char` bounds make a
    /// `Range<char>`, every other bound a `Range<int>`.
    pub fn range_over(bound: &Type, table: &mut CheckerTyTable) -> Self {
        let elem = if bound.apparent(table) == Type::Char {
            Type::Char
        } else {
            Type::Int
        };
        Type::generic(varn_core::BuiltinType::Range.name(), vec![elem], table)
    }

    /// `Range` or `Range<T>`.
    pub fn is_range(&self, table: &CheckerTyTable, interner: &varn_core::AtomInterner) -> bool {
        match table.get(self.0) {
            TypeKind::Builtin(varn_core::BuiltinType::Range) => true,
            TypeKind::Generic(name, _, _) => {
                interner.try_resolve(name) == Some(varn_core::BuiltinType::Range.name())
            }
            _ => false,
        }
    }

    pub fn literal(l: varn_core::TypeLiteral<varn_core::Atom>, table: &mut CheckerTyTable) -> Self {
        Type::resolved(table.intern(TypeKind::Literal(l)))
    }

    pub fn builtin(b: varn_core::BuiltinType, table: &mut CheckerTyTable) -> Self {
        Type::resolved(table.intern(TypeKind::Builtin(b)))
    }

    /// Content-addressed ids are portable, so there is nothing to sanitize:
    /// a foreign id names the same shape here as there (ADR-0012). Kept as a
    /// no-op so callers that still express the old intent keep compiling.
    pub fn sanitize_foreign(self) -> Type {
        self
    }

    // ── Constructors that build a new shape (need `&mut CheckerTyTable`) ──

    pub fn get_array_element_type(&self, table: &CheckerTyTable) -> Type {
        match table.get(self.0) {
            TypeKind::Array(inner) => Type::resolved(inner),
            _ => Type::Dynamic,
        }
    }

    pub fn fn_(f: FunctionType, table: &mut CheckerTyTable) -> Self {
        let fid = table.intern_function(f);
        Type::resolved(table.intern(TypeKind::Fn(fid)))
    }

    pub fn named(name: impl Into<Arc<str>>, table: &mut CheckerTyTable) -> Self {
        let atom = table.intern_name(&name.into());
        Type::resolved(table.intern(TypeKind::Named(atom, None)))
    }

    pub fn named_atom(name: varn_core::Atom, table: &mut CheckerTyTable) -> Self {
        Type::resolved(table.intern(TypeKind::Named(name, None)))
    }

    pub fn named_with_origin_atom(
        name: varn_core::Atom,
        origin: Option<varn_core::Atom>,
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

    /// String-name convenience over [`Self::generic_atom`], no origin.
    pub fn generic(name: impl Into<Arc<str>>, args: Vec<Type>, table: &mut CheckerTyTable) -> Self {
        let atom = table.intern_name(&name.into());
        Type::generic_atom(atom, args, None, table)
    }

    /// String-name convenience over [`Self::generic_atom`], with origin.
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
        name: varn_core::Atom,
        args: Vec<Type>,
        origin: Option<varn_core::Atom>,
        table: &mut CheckerTyTable,
    ) -> Self {
        let ids: Vec<CheckerTyId> = args.iter().map(|a| a.0).collect();
        let list = table.intern_list(&ids);
        Type::resolved(table.intern(TypeKind::Generic(name, list, origin)))
    }

    pub fn object(members: Vec<ObjectTypeMember>, table: &mut CheckerTyTable) -> Self {
        let mid = table.intern_object_members(members);
        Type::resolved(table.intern(TypeKind::Object(mid)))
    }

    pub fn union(members: Vec<Type>, table: &mut CheckerTyTable) -> Self {
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
                        if seen.insert(t) {
                            flat.push(t);
                        }
                    }
                }
                _ => {
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

    // ── Predicates on the fixed intrinsic set (no table access needed) ────

    pub fn is_dynamic(&self) -> bool {
        self.0 == CheckerTyId::DYNAMIC
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

    // ── Everything else needs to read the shape via the table ─────────────

    pub fn stdlib_key<'t>(&self, table: &'t CheckerTyTable) -> Option<&'t str> {
        match table.get(self.0) {
            TypeKind::Primitive(p) => match p {
                varn_core::LangPrimitive::Int => Some(varn_core::LangPrimitive::Int.name()),
                varn_core::LangPrimitive::Float => Some(varn_core::LangPrimitive::Float.name()),
                varn_core::LangPrimitive::Decimal => Some(varn_core::LangPrimitive::Decimal.name()),
                varn_core::LangPrimitive::BigInt => Some(varn_core::LangPrimitive::BigInt.name()),
                varn_core::LangPrimitive::Str => Some(varn_core::LangPrimitive::Str.name()),
                varn_core::LangPrimitive::Char => Some(varn_core::LangPrimitive::Char.name()),
                varn_core::LangPrimitive::Bool => Some(varn_core::LangPrimitive::Bool.name()),
                varn_core::LangPrimitive::Null
                | varn_core::LangPrimitive::Void
                | varn_core::LangPrimitive::Never
                | varn_core::LangPrimitive::Dynamic => None,
            },
            TypeKind::Builtin(varn_core::BuiltinType::Bytes) => {
                Some(varn_core::BuiltinType::Bytes.name())
            }
            TypeKind::Array(_) => Some(varn_core::BuiltinType::Array.name()),
            _ => None,
        }
    }

    pub fn descriptor_key<'t>(
        &self,
        table: &'t CheckerTyTable,
        interner: &'t varn_core::AtomInterner,
    ) -> Option<&'t str> {
        if let Some(k) = self.stdlib_key(table) {
            return Some(k);
        }
        match table.get(self.0) {
            TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => Some(interner.resolve(n)),
            _ => None,
        }
    }
}
