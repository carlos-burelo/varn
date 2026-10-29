use varn_core::TypeLiteral;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LiteralStrId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyListId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeField {
    pub name: std::sync::Arc<str>,
    pub ty: BackendTy,
    pub optional: bool,
    pub readonly: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClassId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnumId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SigId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynReason {
    HostBoundary,

    Union,

    IndexSignature,

    Declared,

    Callable,

    Awaitable(TyId),

    Yields(TyId),

    Range(TyId),

    TaskHandle(TyId),

    CoreMember,

    Spread,

    Interface(ShapeId),

    Shape(ShapeId),

    Record(ShapeId),

    Unrecorded,

    Generic,

    Boxed,

    Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendTy {
    Literal(TypeLiteral<LiteralStrId>),
    Int,
    Float,
    Bool,
    Char,

    Str,
    Bytes,
    Decimal,
    BigInt,
    Array(TyId),
    Map(TyId, TyId),
    Dict(TyId),
    Set(TyId),
    Tuple(TyListId),
    Class(ClassId),
    Enum(EnumId),
    Fn(SigId),

    Nullable(TyId),
    Void,
    Never,
    Dynamic(DynReason),
}

impl BackendTy {
    pub fn representation(self) -> Self {
        match self {
            BackendTy::Literal(TypeLiteral::Int(_)) => BackendTy::Int,
            BackendTy::Literal(TypeLiteral::Str(_)) => BackendTy::Str,
            BackendTy::Literal(TypeLiteral::Bool(_)) => BackendTy::Bool,
            BackendTy::Literal(TypeLiteral::Char(_)) => BackendTy::Char,
            other @ BackendTy::Int
            | other @ BackendTy::Float
            | other @ BackendTy::Bool
            | other @ BackendTy::Char
            | other @ BackendTy::Str
            | other @ BackendTy::Bytes
            | other @ BackendTy::Decimal
            | other @ BackendTy::BigInt
            | other @ BackendTy::Array(_)
            | other @ BackendTy::Map(..)
            | other @ BackendTy::Dict(_)
            | other @ BackendTy::Set(_)
            | other @ BackendTy::Tuple(_)
            | other @ BackendTy::Class(_)
            | other @ BackendTy::Enum(_)
            | other @ BackendTy::Fn(_)
            | other @ BackendTy::Nullable(_)
            | other @ BackendTy::Void
            | other @ BackendTy::Never
            | other @ BackendTy::Dynamic(_) => other,
        }
    }

    pub fn is_boxed(self) -> bool {
        matches!(
            self,
            BackendTy::Decimal
                | BackendTy::BigInt
                | BackendTy::Void
                | BackendTy::Never
                | BackendTy::Dynamic(_)
        )
    }

    pub fn field_kind(self, types: &TyTable) -> Option<varn_core::RuntimeKind> {
        use varn_core::RuntimeKind as T;
        match self {
            BackendTy::Literal(_) => self.representation().field_kind(types),
            BackendTy::Int => Some(T::Int),
            BackendTy::Float => Some(T::Float),
            BackendTy::Bool => Some(T::Bool),
            BackendTy::Str => Some(T::Str),
            BackendTy::Bytes => Some(T::Bytes),
            BackendTy::Char => Some(T::Char),
            BackendTy::Decimal => Some(T::Decimal),
            BackendTy::BigInt => Some(T::BigInt),
            BackendTy::Array(_) => Some(T::Array),
            BackendTy::Set(_) => Some(T::Set),
            BackendTy::Map(..) => Some(T::Map),
            BackendTy::Dict(_) => Some(T::Object),
            BackendTy::Class(_) => Some(T::Class),
            BackendTy::Nullable(_) => {
                let kind = self.non_nullable(types).field_kind(types)?;
                let repr = varn_core::layout::TypeLayout::of_field(Some(kind)).repr;
                (repr == varn_core::layout::ScalarRepr::Ref).then_some(kind)
            }
            BackendTy::Tuple(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(_) => None,
        }
    }

    pub fn non_nullable(self, t: &TyTable) -> BackendTy {
        self.non_nullable_with_depth(t, 0)
    }

    fn non_nullable_with_depth(self, t: &TyTable, depth: usize) -> BackendTy {
        const DEPTH_LIMIT: usize = 32;
        match self {
            BackendTy::Nullable(inner) if depth < DEPTH_LIMIT && t.contains(inner) => {
                t.get(inner).non_nullable_with_depth(t, depth + 1)
            }
            other @ BackendTy::Literal(_)
            | other @ BackendTy::Int
            | other @ BackendTy::Float
            | other @ BackendTy::Bool
            | other @ BackendTy::Char
            | other @ BackendTy::Str
            | other @ BackendTy::Bytes
            | other @ BackendTy::Decimal
            | other @ BackendTy::BigInt
            | other @ BackendTy::Array(_)
            | other @ BackendTy::Map(..)
            | other @ BackendTy::Dict(_)
            | other @ BackendTy::Set(_)
            | other @ BackendTy::Tuple(_)
            | other @ BackendTy::Class(_)
            | other @ BackendTy::Enum(_)
            | other @ BackendTy::Fn(_)
            | other @ BackendTy::Nullable(_)
            | other @ BackendTy::Void
            | other @ BackendTy::Never
            | other @ BackendTy::Dynamic(_) => other,
        }
    }

    pub fn is_unboxed_scalar(self) -> bool {
        matches!(
            self.representation(),
            BackendTy::Int | BackendTy::Float | BackendTy::Bool | BackendTy::Char
        )
    }
}

#[derive(Debug, Default, Clone)]
pub struct TyTable {
    literal_strings: Vec<std::sync::Arc<str>>,
    entries: Vec<BackendTy>,
    dedup: rustc_hash::FxHashMap<BackendTy, u32>,
    lists: Vec<Vec<BackendTy>>,
    shapes: Vec<Vec<ShapeField>>,
    shape_names: Vec<Option<std::sync::Arc<str>>>,
    named_shapes: rustc_hash::FxHashMap<usize, u32>,
    open_aliases: Vec<std::sync::Arc<str>>,
}

impl TyTable {
    pub fn intern_literal_str(&mut self, text: &str) -> LiteralStrId {
        if let Some(at) = self.literal_strings.iter().position(|s| s.as_ref() == text) {
            return LiteralStrId(at as u32);
        }
        let id = LiteralStrId(self.literal_strings.len() as u32);
        self.literal_strings.push(std::sync::Arc::from(text));
        id
    }

    pub fn literal_str(&self, id: LiteralStrId) -> Option<&str> {
        self.literal_strings.get(id.0 as usize).map(|s| s.as_ref())
    }

    pub fn intern(&mut self, ty: BackendTy) -> TyId {
        if let Some(&i) = self.dedup.get(&ty) {
            return TyId(i);
        }
        let i = self.entries.len() as u32;
        self.entries.push(ty);
        self.dedup.insert(ty, i);
        TyId(i)
    }

    pub fn get(&self, id: TyId) -> BackendTy {
        self.entries[id.0 as usize]
    }

    pub fn intern_list(&mut self, tys: &[BackendTy]) -> TyListId {
        if let Some(i) = self.lists.iter().position(|l| l.as_slice() == tys) {
            return TyListId(i as u32);
        }
        let i = self.lists.len() as u32;
        self.lists.push(tys.to_vec());
        TyListId(i)
    }

    pub fn get_list(&self, id: TyListId) -> &[BackendTy] {
        &self.lists[id.0 as usize]
    }

    pub fn contains(&self, id: TyId) -> bool {
        (id.0 as usize) < self.entries.len()
    }

    pub fn contains_list(&self, id: TyListId) -> bool {
        (id.0 as usize) < self.lists.len()
    }

    pub fn enter_alias(&mut self, name: &str) -> bool {
        if self.open_aliases.iter().any(|open| open.as_ref() == name) {
            return false;
        }
        self.open_aliases.push(std::sync::Arc::from(name));
        true
    }

    pub fn leave_alias(&mut self) {
        self.open_aliases.pop();
    }

    pub fn intern_shape(&mut self, fields: Vec<ShapeField>) -> ShapeId {
        let structural = self
            .shapes
            .iter()
            .zip(&self.shape_names)
            .position(|(known, name)| name.is_none() && *known == fields);
        if let Some(i) = structural {
            return ShapeId(i as u32);
        }
        self.shapes.push(fields);
        self.shape_names.push(None);
        ShapeId(self.shapes.len() as u32 - 1)
    }

    pub fn named_shape(&self, key: usize) -> Option<ShapeId> {
        self.named_shapes.get(&key).map(|&i| ShapeId(i))
    }

    pub fn reserve_named_shape(&mut self, key: usize, name: std::sync::Arc<str>) -> ShapeId {
        let id = self.shapes.len() as u32;
        self.shapes.push(Vec::new());
        self.shape_names.push(Some(name));
        self.named_shapes.insert(key, id);
        ShapeId(id)
    }

    pub fn fill_named_shape(&mut self, id: ShapeId, fields: Vec<ShapeField>) {
        self.shapes[id.0 as usize] = fields;
    }

    pub fn shape_name(&self, id: ShapeId) -> Option<&std::sync::Arc<str>> {
        self.shape_names.get(id.0 as usize)?.as_ref()
    }

    pub fn get_shape(&self, id: ShapeId) -> &[ShapeField] {
        &self.shapes[id.0 as usize]
    }

    pub fn contains_shape(&self, id: ShapeId) -> bool {
        (id.0 as usize) < self.shapes.len()
    }
}
