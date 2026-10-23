use super::super::ir::Value;
use crate::hir::{HirBinOp, HirUnOp};
use varn_tir::{BackendTy, DynReason};

pub(super) fn args_list(args: &[Value]) -> String {
    if args.is_empty() {
        String::new()
    } else {
        let inner = args.iter().map(|v| val(*v)).collect::<Vec<_>>().join(", ");
        format!("({inner})")
    }
}

pub(super) fn val(v: Value) -> String {
    format!("v{}", v.0)
}

pub(super) fn ty(t: BackendTy) -> &'static str {
    match t {
        BackendTy::Int => varn_core::LangPrimitive::Int.name(),
        BackendTy::Float => varn_core::LangPrimitive::Float.name(),
        BackendTy::Bool => varn_core::LangPrimitive::Bool.name(),
        BackendTy::Str => varn_core::LangPrimitive::Str.name(),
        BackendTy::Dynamic(DynReason::Runtime) => "ref",
        BackendTy::Dynamic(_) => "dyn",
        BackendTy::Char => "char",
        BackendTy::Bytes => "bytes",
        BackendTy::Decimal => "decimal",
        BackendTy::BigInt => "bigint",
        BackendTy::Enum(_) => "enum",
        BackendTy::Fn(_) => "fn",
        BackendTy::Void => "void",
        BackendTy::Never => "never",

        BackendTy::Array(_) => "array",
        BackendTy::Map(..) | BackendTy::Dict(_) => "map",
        BackendTy::Set(_) => "set",
        BackendTy::Tuple(_) => "tuple",
        BackendTy::Class(_) => "class",
        BackendTy::Nullable(_) => "nullable",
    }
}

pub(super) fn binop(op: HirBinOp) -> &'static str {
    match op {
        HirBinOp::Add => "add",
        HirBinOp::Sub => "sub",
        HirBinOp::Mul => "mul",
        HirBinOp::Div => "div",
        HirBinOp::Mod => "mod",
        HirBinOp::Pow => "pow",
        HirBinOp::Eq => "eq",
        HirBinOp::Ne => "ne",
        HirBinOp::Lt => "lt",
        HirBinOp::Le => "le",
        HirBinOp::Gt => "gt",
        HirBinOp::Ge => "ge",
        HirBinOp::BitAnd => "band",
        HirBinOp::BitOr => "bor",
        HirBinOp::BitXor => "bxor",
        HirBinOp::Shl => "shl",
        HirBinOp::Shr => "shr",
        HirBinOp::Ushr => "ushr",
        HirBinOp::Instanceof => "instanceof",
        HirBinOp::In => "in",
    }
}

pub(super) fn unop(op: HirUnOp) -> &'static str {
    match op {
        HirUnOp::Neg => "neg",
        HirUnOp::Not => "not",
        HirUnOp::BitNot => "bnot",
        HirUnOp::Typeof => "typeof",
    }
}
