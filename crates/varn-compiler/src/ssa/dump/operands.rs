use super::super::ir::Value;
use crate::hir::{HirBinOp, HirType, HirUnOp};

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

pub(super) fn ty(t: HirType) -> &'static str {
    match t {
        HirType::Int => varn_core::LangPrimitive::Int.name(),
        HirType::Float => varn_core::LangPrimitive::Float.name(),
        HirType::Bool => varn_core::LangPrimitive::Bool.name(),
        HirType::Str => varn_core::LangPrimitive::Str.name(),
        HirType::Ref => "ref",
        HirType::Dynamic => "dyn",

        HirType::Array(_) => "array",
        HirType::Map(_, _) => "map",
        HirType::Set(_) => "set",
        HirType::Class(_) => "class",
        HirType::Nullable(_) => "nullable",
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
