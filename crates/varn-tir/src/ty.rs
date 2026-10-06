#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyListId(pub u32);

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

    NotYetSupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendTy {
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
    pub fn field_kind(self, types: &TyTable) -> Option<varn_core::RuntimeKind> {
        use varn_core::RuntimeKind as T;
        match self {
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
            other => other,
        }
    }

    pub fn is_unboxed_scalar(self) -> bool {
        matches!(
            self,
            BackendTy::Int | BackendTy::Float | BackendTy::Bool | BackendTy::Char
        )
    }
}

#[derive(Debug, Default)]
pub struct TyTable {
    entries: Vec<BackendTy>,
    dedup: rustc_hash::FxHashMap<BackendTy, u32>,
    lists: Vec<Vec<BackendTy>>,
}

impl TyTable {
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
}
