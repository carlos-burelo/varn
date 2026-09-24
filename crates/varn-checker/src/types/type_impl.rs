use super::*;
use crate::module_resolver::ImportResolver;
use std::sync::Arc;

#[allow(non_upper_case_globals)]
impl Type {
    // ── Intrinsic constants ──────────────────────────────────────────────
    //
    // `CheckerTyTable::new` pre-seeds every one of these shapes at a FIXED
    // `CheckerTyId` (see `interned.rs`), so these stay `const` even though
    // `Type` now wraps a hash-consed id instead of an owned tree: no table
    // access is needed to produce them, only to look one up later.
    pub const Int: Type = Type(CheckerTyId::INT, false);
    pub const Float: Type = Type(CheckerTyId::FLOAT, false);
    pub const Decimal: Type = Type(CheckerTyId::DECIMAL, false);
    pub const BigInt: Type = Type(CheckerTyId::BIGINT, false);
    pub const Str: Type = Type(CheckerTyId::STR, false);
    pub const Char: Type = Type(CheckerTyId::CHAR, false);
    pub const Bool: Type = Type(CheckerTyId::BOOL, false);
    pub const Void: Type = Type(CheckerTyId::VOID, false);
    pub const Null: Type = Type(CheckerTyId::NULL, false);
    pub const Never: Type = Type(CheckerTyId::NEVER, false);
    pub const Dynamic: Type = Type(CheckerTyId::DYNAMIC, false);
    pub const This: Type = Type(CheckerTyId::THIS, false);

    /// Primitives have fixed ids (the constants above); interning goes through
    /// the table so every spelling of one lands on the same id.
    pub fn primitive(p: varn_core::LangPrimitive, table: &mut CheckerTyTable) -> Self {
        Type(table.intern(TypeKind::Primitive(p)), false)
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
                P::Null
                | P::Float
                | P::BigInt
                | P::Decimal
                | P::Void
                | P::Never
                | P::Dynamic => Type::Dynamic,
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

    pub fn literal(l: varn_core::TypeLiteral<varn_core::Atom>, table: &mut CheckerTyTable) -> Self {
        Type(table.intern(TypeKind::Literal(l)), false)
    }

    pub fn builtin(b: varn_core::BuiltinType, table: &mut CheckerTyTable) -> Self {
        Type(table.intern(TypeKind::Builtin(b)), false)
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
            TypeKind::Array(inner) => Type(inner, false),
            _ => Type::Dynamic,
        }
    }

    pub fn fn_(f: FunctionType, table: &mut CheckerTyTable) -> Self {
        let fid = table.intern_function(f);
        Type(table.intern(TypeKind::Fn(fid)), false)
    }

    pub fn named(
        name: impl Into<Arc<str>>,
        resolver: &dyn ImportResolver,
        table: &mut CheckerTyTable,
    ) -> Self {
        let atom = resolver.intern(&name.into());
        Type(table.intern(TypeKind::Named(atom, None)), false)
    }

    pub fn named_atom(name: varn_core::Atom, table: &mut CheckerTyTable) -> Self {
        Type(table.intern(TypeKind::Named(name, None)), false)
    }

    pub fn named_with_origin_atom(
        name: varn_core::Atom,
        origin: Option<varn_core::Atom>,
        table: &mut CheckerTyTable,
    ) -> Self {
        Type(table.intern(TypeKind::Named(name, origin)), false)
    }

    /// String-name convenience over [`Self::named_with_origin_atom`]: mints
    /// both `Atom`s via `resolver.intern` — the shared, per-compilation
    /// interner every other `Named`/`Generic` name comes from — rather than
    /// asking every call site to intern for itself.
    pub fn named_with_origin(
        name: impl Into<Arc<str>>,
        origin: Option<Arc<str>>,
        resolver: &dyn ImportResolver,
        table: &mut CheckerTyTable,
    ) -> Self {
        let name_atom = resolver.intern(&name.into());
        let origin_atom = origin.map(|o| resolver.intern(&o));
        Type::named_with_origin_atom(name_atom, origin_atom, table)
    }

    /// String-name convenience over [`Self::generic_atom`], no origin.
    pub fn generic(
        name: impl Into<Arc<str>>,
        args: Vec<Type>,
        resolver: &dyn ImportResolver,
        table: &mut CheckerTyTable,
    ) -> Self {
        let atom = resolver.intern(&name.into());
        Type::generic_atom(atom, args, None, table)
    }

    /// String-name convenience over [`Self::generic_atom`], with origin.
    pub fn generic_with_origin(
        name: impl Into<Arc<str>>,
        args: Vec<Type>,
        origin: Option<Arc<str>>,
        resolver: &dyn ImportResolver,
        table: &mut CheckerTyTable,
    ) -> Self {
        let atom = resolver.intern(&name.into());
        let origin_atom = origin.map(|o| resolver.intern(&o));
        Type::generic_atom(atom, args, origin_atom, table)
    }

    pub fn array(inner: Type, table: &mut CheckerTyTable) -> Self {
        Type(table.intern(TypeKind::Array(inner.0)), false)
    }

    pub fn generic_atom(
        name: varn_core::Atom,
        args: Vec<Type>,
        origin: Option<varn_core::Atom>,
        table: &mut CheckerTyTable,
    ) -> Self {
        let ids: Vec<CheckerTyId> = args.iter().map(|a| a.0).collect();
        let list = table.intern_list(&ids);
        Type(table.intern(TypeKind::Generic(name, list, origin)), false)
    }

    pub fn object(members: Vec<ObjectTypeMember>, table: &mut CheckerTyTable) -> Self {
        let mid = table.intern_object_members(members);
        Type(table.intern(TypeKind::Object(mid)), false)
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
                    return Type(table.intern(TypeKind::Union(list)), false);
                }
            }
        } else if members.is_empty() {
            let list = table.intern_list(&[]);
            return Type(table.intern(TypeKind::Union(list)), false);
        }

        let mut seen = rustc_hash::FxHashSet::default();
        let mut flat: Vec<Type> = Vec::with_capacity(members.len());
        for m in members {
            match table.get(m.0).clone() {
                TypeKind::Union(inner_list) => {
                    for id in table.get_list(inner_list).to_vec() {
                        let t = Type(id, false);
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
            Type(table.intern(TypeKind::Union(list)), false)
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
            TypeKind::Builtin(varn_core::BuiltinType::Bytes) => Some(varn_core::BuiltinType::Bytes.name()),
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

    /// `Bytes` has no fixed `CheckerTyId` (only the seeded intrinsics do),
    /// so this reads the shape via the table — unlike `is_str`/`is_bool`
    /// which compare fixed ids directly. Ported from main's `is_bytes`
    /// (canonical bytes, ADR-0008); main callers using `is_bytes()` with no
    /// args must pass the table.
    pub fn is_bytes(&self, table: &CheckerTyTable) -> bool {
        matches!(table.get(self.0), TypeKind::Builtin(varn_core::BuiltinType::Bytes))
    }

    pub fn is_nullable(&self, table: &CheckerTyTable) -> bool {
        match table.get(self.0) {
            TypeKind::Primitive(varn_core::LangPrimitive::Null) => true,
            TypeKind::Union(list) => table
                .get_list(list)
                .iter()
                .any(|m| Type(*m, false).is_nullable(table)),
            _ => false,
        }
    }

    pub fn make_nullable(ty: Type, table: &mut CheckerTyTable) -> Type {
        if ty.is_nullable(table) {
            ty
        } else {
            Type::union(vec![ty, Type::Null], table)
        }
    }

    pub fn non_nullified(&self, table: &mut CheckerTyTable) -> Type {
        match table.get(self.0).clone() {
            TypeKind::Primitive(varn_core::LangPrimitive::Null) => Type::Never,
            TypeKind::Union(list) => {
                let members: Vec<Type> = table
                    .get_list(list)
                    .iter()
                    .map(|id| Type(*id, false))
                    .collect();
                let new_members: Vec<Type> = members
                    .into_iter()
                    .filter(|m| !m.is_nullable(table))
                    .collect();

                if new_members.is_empty() {
                    Type::Never
                } else if new_members.len() == 1 {
                    new_members.into_iter().next().unwrap()
                } else {
                    let ids: Vec<CheckerTyId> = new_members.iter().map(|m| m.0).collect();
                    let new_list = table.intern_list(&ids);
                    Type(table.intern(TypeKind::Union(new_list)), false)
                }
            }
            _ => *self,
        }
    }

    pub fn minus_named(&self, name: varn_core::Atom, table: &mut CheckerTyTable) -> Type {
        match table.get(self.0).clone() {
            TypeKind::Named(n, _) if n == name => Type::Never,
            TypeKind::Union(list) => {
                let members: Vec<CheckerTyId> = table.get_list(list).to_vec();
                let mut kept: Vec<CheckerTyId> = Vec::with_capacity(members.len());
                for id in members {
                    let is_named_match =
                        matches!(table.get(id), TypeKind::Named(n, _) if n == name);
                    if !is_named_match {
                        kept.push(id);
                    }
                }
                if kept.is_empty() {
                    Type::Never
                } else if kept.len() == 1 {
                    Type(kept[0], false)
                } else {
                    let new_list = table.intern_list(&kept);
                    Type(table.intern(TypeKind::Union(new_list)), false)
                }
            }
            _ => *self,
        }
    }

    pub fn minus(&self, other: &Type, table: &mut CheckerTyTable) -> Type {
        if self == other {
            return Type::Never;
        }
        match table.get(self.0).clone() {
            TypeKind::Union(list) => {
                let members: Vec<CheckerTyId> = table.get_list(list).to_vec();
                let dynamic_array = {
                    let arr = Type::array(Type::Dynamic, table);
                    arr.0
                };
                let mut kept: Vec<CheckerTyId> = Vec::with_capacity(members.len());
                for id in members {
                    if id == other.0 {
                        continue;
                    }
                    if let (TypeKind::Array(_), TypeKind::Array(_)) =
                        (table.get(id), table.get(other.0))
                    {
                        if other.0 == dynamic_array || id == other.0 {
                            continue;
                        }
                    }
                    kept.push(id);
                }
                if kept.is_empty() {
                    Type::Never
                } else if kept.len() == 1 {
                    Type(kept[0], false)
                } else {
                    let new_list = table.intern_list(&kept);
                    Type(table.intern(TypeKind::Union(new_list)), false)
                }
            }
            _ => *self,
        }
    }

    pub fn map_generics(
        &self,
        mapping: &FxHashMap<varn_core::Atom, Type>,
        table: &mut CheckerTyTable,
    ) -> Type {
        match table.get(self.0).clone() {
            TypeKind::Named(n, _) => {
                if let Some(t) = mapping.get(&n) {
                    return *t;
                }
                *self
            }
            TypeKind::Generic(n, args, origin) => {
                let arg_ids = table.get_list(args).to_vec();
                let new_args: Vec<CheckerTyId> = arg_ids
                    .into_iter()
                    .map(|a| Type(a, false).map_generics(mapping, table).0)
                    .collect();
                let new_list = table.intern_list(&new_args);
                Type(table.intern(TypeKind::Generic(n, new_list, origin)), self.1)
            }
            TypeKind::Array(inner) => {
                let mapped = Type(inner, false).map_generics(mapping, table);
                Type::array(mapped, table)
            }
            TypeKind::Union(list) => {
                let ids = table.get_list(list).to_vec();
                let new_members: Vec<Type> = ids
                    .into_iter()
                    .map(|id| Type(id, false).map_generics(mapping, table))
                    .collect();
                Type::union(new_members, table)
            }
            TypeKind::Fn(fid) => {
                let ft = table.get_function(fid).clone();
                let new_params: Vec<FunctionParam> = ft
                    .params
                    .iter()
                    .map(|p| FunctionParam {
                        name: p.name.clone(),
                        ty: Type(p.ty, false).map_generics(mapping, table).0,
                        optional: p.optional,
                        is_rest: p.is_rest,
                    })
                    .collect();
                let new_ret = Type(ft.return_type, false).map_generics(mapping, table).0;
                Type::fn_(
                    FunctionType {
                        params: new_params,
                        return_type: new_ret,
                        is_arrow: ft.is_arrow,
                        type_params: ft.type_params.clone(),
                    },
                    table,
                )
            }
            TypeKind::Object(mid) => {
                let members = table.get_object_members(mid).to_vec();
                let new_members: Vec<ObjectTypeMember> = members
                    .into_iter()
                    .map(|m| m.map_generics(mapping, table))
                    .collect();
                Type::object(new_members, table)
            }
            _ => *self,
        }
    }

    pub fn with_origin(self, origin: varn_core::Atom, table: &mut CheckerTyTable) -> Self {
        match table.get(self.0).clone() {
            TypeKind::Named(n, _) => Type(table.intern(TypeKind::Named(n, Some(origin))), self.1),
            TypeKind::Generic(n, args, _) => Type(
                table.intern(TypeKind::Generic(n, args, Some(origin))),
                self.1,
            ),
            _ => self,
        }
    }
}
