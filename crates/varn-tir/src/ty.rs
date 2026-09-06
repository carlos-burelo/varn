//! The one type the backend speaks.
//!
//! It replaces the CgTy → HirType → SlotKind cascade, where each narrowing
//! silently dropped constructors: `char`, `decimal` and `bigint` died at HIR
//! despite having a TypeTag and a runtime representation, and `T?` collapsed
//! to Dynamic, which is why a nullable scalar could never be a (value, bit)
//! pair.
//!
//! Deliberately NOT `Default`: "the type you get when you wrote none" is the
//! hole this IR exists to close. `varn_checker::types::Type` has one, and it
//! is `Dynamic`.

/// Handle into a [`TyTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyId(pub u32);

/// Handle to a sequence of types — tuple elements, signature parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyListId(pub u32);

/// Handle into the module's class table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClassId(pub u32);

/// Handle into the module's enum table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnumId(pub u32);

/// Handle into the module's signature table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SigId(pub u32);

/// Handle to a function the module can call directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnId(pub u32);

/// Handle to an imported module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(pub u32);

/// A local binding within one function body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

/// Why a value is dynamic. A total count is not actionable; a count per
/// reason is — it separates the host boundary, which is honest, from an
/// inference hole, which is a bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynReason {
    /// Crosses the host boundary: a native return, `JSON.parse`.
    HostBoundary,
    /// A non-discriminated union. Representing these is deliberately deferred.
    Union,
    /// Read off an index signature — `{ [key: str]: T }` has no named members.
    IndexSignature,
    /// The author wrote no annotation and inference reached no answer.
    Unannotated,
    /// The TIR cannot express this type yet. This is the redesign's backlog.
    NotYetSupported,
}

/// The type of a value, as the backend sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendTy {
    // Scalars — travel in a register, untagged.
    Int,
    Float,
    Bool,
    Char,
    // References with a known type.
    Str,
    Decimal,
    BigInt,
    Array(TyId),
    Map(TyId, TyId),
    Set(TyId),
    Tuple(TyListId),
    Class(ClassId),
    Enum(EnumId),
    Fn(SigId),
    /// `T?` — the payload plus null. Keeps the payload: over a reference this
    /// is the null pattern, over a scalar a (value, bit) pair.
    Nullable(TyId),
    Void,
    Never,
    Dynamic(DynReason),
}

impl BackendTy {
    /// The type with nullability stripped, for consumers that guard null
    /// separately. Needs the table because the payload is behind a handle.
    pub fn non_nullable(self, t: &TyTable) -> BackendTy {
        match self {
            BackendTy::Nullable(inner) => t.get(inner).non_nullable(t),
            other => other,
        }
    }

    /// Whether a value of this type fits in one machine register with no tag.
    pub fn is_unboxed_scalar(self) -> bool {
        matches!(
            self,
            BackendTy::Int | BackendTy::Float | BackendTy::Bool | BackendTy::Char
        )
    }
}

/// Per-module interning table for the structured types.
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

    /// Whether `id` names an entry this table holds. The verifier's
    /// well-formedness check calls this on every handle it meets.
    pub fn contains(&self, id: TyId) -> bool {
        (id.0 as usize) < self.entries.len()
    }

    pub fn contains_list(&self, id: TyListId) -> bool {
        (id.0 as usize) < self.lists.len()
    }
}
