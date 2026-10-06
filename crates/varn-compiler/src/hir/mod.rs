use std::sync::Arc;


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyId(pub u32);


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
    
    Array(TyId),
    Map(TyId, TyId),
    Set(TyId),
    
    Class(ClassId),
    
    Nullable(TyId),
}




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
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,

    Ushr,

    Instanceof,

    In,
}




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
