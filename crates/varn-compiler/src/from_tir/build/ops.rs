use crate::hir::{HirBinOp, HirUnOp, HirUpvalueSrc, LocalId};
use varn_tir::{BackendTy, TirBinOp, TirUnOp};

pub(super) fn bin_op(op: TirBinOp) -> HirBinOp {
    match op {
        TirBinOp::Add => HirBinOp::Add,
        TirBinOp::Sub => HirBinOp::Sub,
        TirBinOp::Mul => HirBinOp::Mul,
        TirBinOp::Div => HirBinOp::Div,
        TirBinOp::Mod => HirBinOp::Mod,
        TirBinOp::Pow => HirBinOp::Pow,
        TirBinOp::Eq => HirBinOp::Eq,
        TirBinOp::Ne => HirBinOp::Ne,
        TirBinOp::Lt => HirBinOp::Lt,
        TirBinOp::Le => HirBinOp::Le,
        TirBinOp::Gt => HirBinOp::Gt,
        TirBinOp::Ge => HirBinOp::Ge,
        TirBinOp::BitAnd => HirBinOp::BitAnd,
        TirBinOp::BitOr => HirBinOp::BitOr,
        TirBinOp::BitXor => HirBinOp::BitXor,
        TirBinOp::Shl => HirBinOp::Shl,
        TirBinOp::Shr => HirBinOp::Shr,
        TirBinOp::Ushr => HirBinOp::Ushr,
        TirBinOp::Instanceof => HirBinOp::Instanceof,
        TirBinOp::In => HirBinOp::In,
    }
}

pub(super) fn un_op(op: TirUnOp) -> HirUnOp {
    match op {
        TirUnOp::Neg => HirUnOp::Neg,
        TirUnOp::Not => HirUnOp::Not,
        TirUnOp::BitNot => HirUnOp::BitNot,
        TirUnOp::Typeof => HirUnOp::Typeof,
        TirUnOp::IsNull => unreachable!("handled by lower_expr"),
    }
}

pub(super) fn upvalue_src(u: varn_tir::TirUpvalue) -> HirUpvalueSrc {
    match u {
        varn_tir::TirUpvalue::ParentLocal(i) => HirUpvalueSrc::ParentLocal(LocalId(i)),
        varn_tir::TirUpvalue::ParentParam(i) => HirUpvalueSrc::ParentParam(i),
        varn_tir::TirUpvalue::ParentUpvalue(i) => HirUpvalueSrc::ParentUpvalue(i),
    }
}

pub(super) fn is_free_fn_name(name: &str) -> bool {
    let mut cs = name.chars();
    matches!(cs.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && cs.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

pub(super) fn numeric_domain(bt: BackendTy) -> Option<varn_core::NumericDomain> {
    use varn_core::NumericDomain as D;
    Some(match bt {
        BackendTy::Int => D::Int,
        BackendTy::Float => D::Float,
        BackendTy::BigInt => D::BigInt,
        BackendTy::Decimal => D::Decimal,
        _ => return None,
    })
}

pub(super) fn field_kind(
    bt: BackendTy,
    types: &varn_tir::TyTable,
) -> Option<varn_core::RuntimeKind> {
    use varn_core::RuntimeKind as T;
    match bt {
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
            let kind = field_kind(bt.non_nullable(types), types)?;
            let repr = varn_types::layout::TypeLayout::of_field(Some(kind)).repr;
            (repr == varn_types::layout::ScalarRepr::Ref).then_some(kind)
        }
        BackendTy::Tuple(_)
        | BackendTy::Enum(_)
        | BackendTy::Fn(_)
        | BackendTy::Void
        | BackendTy::Never
        | BackendTy::Dynamic(_) => None,
    }
}
