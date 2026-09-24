use std::sync::Arc;

/// Interned handle to a nested `HirType` in the module's [`TyTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyId(pub u32);

/// Interned handle to a class name in the module's [`TyTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClassId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirType {
    Int,
    Float,
    Bool,
    Str,
    Ref,
    Dynamic,
    /// `Array<T>`; element type behind a [`TyId`] to keep `HirType` `Copy`.
    Array(TyId),
    Map(TyId, TyId),
    Set(TyId),
    /// Instance of a source-declared class.
    Class(ClassId),
    /// `T?` — payload type plus null.
    Nullable(TyId),
}

/// Module-wide intern table resolving the [`TyId`]/[`ClassId`] handles that
/// structured [`HirType`]s carry. One per lowered module; shared onward so
/// SSA passes and emission can resolve nesting.
#[derive(Debug, Default)]
pub struct TyTable {
    entries: Vec<HirType>,
    dedup: rustc_hash::FxHashMap<HirType, u32>,
    class_names: Vec<Arc<str>>,
    class_dedup: rustc_hash::FxHashMap<Arc<str>, u32>,
}

impl TyTable {
    pub fn intern(&mut self, ty: HirType) -> TyId {
        if let Some(&i) = self.dedup.get(&ty) {
            return TyId(i);
        }
        let i = self.entries.len() as u32;
        self.entries.push(ty);
        self.dedup.insert(ty, i);
        TyId(i)
    }

    pub fn get(&self, id: TyId) -> HirType {
        self.entries[id.0 as usize]
    }

    pub fn class_id(&mut self, name: &Arc<str>) -> ClassId {
        if let Some(&i) = self.class_dedup.get(name) {
            return ClassId(i);
        }
        let i = self.class_names.len() as u32;
        self.class_names.push(name.clone());
        self.class_dedup.insert(name.clone(), i);
        ClassId(i)
    }

    pub fn class_name(&self, id: ClassId) -> &Arc<str> {
        &self.class_names[id.0 as usize]
    }

    /// Import a checker-side [`varn_core::CgTy`] projection. Kinds the
    /// backend has no precise representation for yet stay `Dynamic` —
    /// conservative, never wrong; heap-boxed scalars (char/decimal/bigint)
    /// project to `Ref`, matching `from_tir::ty::lower`.
    pub fn from_cg(&mut self, cg: &varn_core::CgTy) -> HirType {
        use varn_core::CgTy;
        match cg {
            CgTy::Int => HirType::Int,
            CgTy::Float => HirType::Float,
            CgTy::Bool => HirType::Bool,
            CgTy::Str => HirType::Str,
            CgTy::Array(el) => {
                let e = self.from_cg(el);
                let id = self.intern(e);
                HirType::Array(id)
            }
            CgTy::Map(k, v) => {
                let kt = self.from_cg(k);
                let vt = self.from_cg(v);
                let ki = self.intern(kt);
                let vi = self.intern(vt);
                HirType::Map(ki, vi)
            }
            CgTy::Set(el) => {
                let e = self.from_cg(el);
                let id = self.intern(e);
                HirType::Set(id)
            }
            CgTy::Class(name) => HirType::Class(self.class_id(name)),
            CgTy::Nullable(inner) => {
                let t = self.from_cg(inner);
                if t == HirType::Dynamic {
                    HirType::Dynamic
                } else {
                    let id = self.intern(t);
                    HirType::Nullable(id)
                }
            }
            // A function value is a closure or a native — either way a heap
            // reference, never an immediate. `Ref` is the honest projection
            // and it is what keeps every read of a module function from
            // landing in a `Dynamic` register.
            CgTy::Fn => HirType::Ref,
            // `char` vive en el heap (`HeapObj::Char`): `Ref` es la proyección
            // honesta y coincide con `from_tir::ty::lower`. `decimal`/`bigint`
            // son heap con ensanchado `int` y tolerancia `dynamic`: `Dynamic`
            // los aloja sin allocar (también como `from_tir::ty::lower`).
            CgTy::Char => HirType::Ref,
            CgTy::Decimal | CgTy::BigInt | CgTy::Dynamic => HirType::Dynamic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirUpvalueSrc {
    ParentLocal(LocalId),

    ParentParam(u32),

    ParentUpvalue(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,

    Ushr,

    Instanceof,

    In,
}

/// Result type of a `Binary` node whose `ty` field holds the OPERAND class.
/// Comparisons produce `Bool`; arithmetic keeps the operand class
/// (`varn_core::numeric`).
pub(crate) fn binary_result_ty(op: HirBinOp, operand_ty: HirType) -> HirType {
    use HirBinOp::*;
    match op {
        Eq | Ne | Lt | Le | Gt | Ge | Instanceof | In => HirType::Bool,
        _ => operand_ty,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirUnOp {
    Neg,
    Not,
    BitNot,
    Typeof,
}
