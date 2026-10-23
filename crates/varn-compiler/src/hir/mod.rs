use varn_tir::BackendTy;

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

pub(crate) fn binary_result_ty(op: HirBinOp, operand_ty: BackendTy) -> BackendTy {
    use HirBinOp::*;
    match op {
        Eq | Ne | Lt | Le | Gt | Ge | Instanceof | In => BackendTy::Bool,
        Add | Sub | Mul | Div | Mod | Pow | BitAnd | BitOr | BitXor | Shl | Shr | Ushr => {
            operand_ty
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirUnOp {
    Neg,
    Not,
    BitNot,
    Typeof,
}
