use std::rc::Rc;

use varn_core::OpCode;
use varn_types::chunk::FunctionProto;

use crate::hir::*;
use crate::OptError;

pub(crate) fn lower_function(f: &HirFunction, source_file: Rc<str>) -> FunctionProto {
    match crate::ssa::try_compile_function(f, source_file) {
        Ok(proto) => proto,
        Err(OptError::Unsupported(why)) => {
            panic!(
                "SSA compiler unsupported construct in function {}: {}",
                f.name, why
            );
        }
    }
}

pub(crate) fn bin_opcode(op: HirBinOp, ty: HirType) -> OpCode {
    use HirBinOp::*;
    match ty {
        HirType::Int => match op {
            Add => OpCode::AddInt,
            Sub => OpCode::SubInt,
            Mul => OpCode::MulInt,
            Div => OpCode::DivInt,
            Mod => OpCode::ModInt,
            Pow => OpCode::PowInt,
            Eq => OpCode::EqInt,
            Ne => OpCode::NeqInt,
            Lt => OpCode::LtInt,
            Le => OpCode::LteInt,
            Gt => OpCode::GtInt,
            Ge => OpCode::GteInt,
            BitAnd => OpCode::BitAnd,
            BitOr => OpCode::BitOr,
            BitXor => OpCode::BitXor,
            Shl => OpCode::Shl,
            Shr => OpCode::Shr,
            Ushr => OpCode::Ushr,
            Instanceof => OpCode::Instanceof,
            In => OpCode::In,
            And | Or => OpCode::Add,
        },
        HirType::Float => match op {
            Add => OpCode::AddFloat,
            Sub => OpCode::SubFloat,
            Mul => OpCode::MulFloat,
            Div => OpCode::DivFloat,
            Mod => OpCode::ModFloat,
            Pow => OpCode::PowFloat,
            Eq => OpCode::EqFloat,
            Ne => OpCode::NeqFloat,
            Lt => OpCode::LtFloat,
            Le => OpCode::LteFloat,
            Gt => OpCode::GtFloat,
            Ge => OpCode::GteFloat,
            BitAnd => OpCode::BitAnd,
            BitOr => OpCode::BitOr,
            BitXor => OpCode::BitXor,
            Shl => OpCode::Shl,
            Shr => OpCode::Shr,
            Ushr => OpCode::Ushr,
            Instanceof => OpCode::Instanceof,
            In => OpCode::In,
            And | Or => OpCode::Add,
        },
        _ => match op {
            Add => OpCode::Add,
            Sub => OpCode::Sub,
            Mul => OpCode::Mul,
            Div => OpCode::Div,
            Mod => OpCode::Mod,
            Pow => OpCode::Pow,
            Eq => OpCode::Eq,
            Ne => OpCode::Neq,
            Lt => OpCode::Lt,
            Le => OpCode::Lte,
            Gt => OpCode::Gt,
            Ge => OpCode::Gte,
            BitAnd => OpCode::BitAnd,
            BitOr => OpCode::BitOr,
            BitXor => OpCode::BitXor,
            Shl => OpCode::Shl,
            Shr => OpCode::Shr,
            Ushr => OpCode::Ushr,
            Instanceof => OpCode::Instanceof,
            In => OpCode::In,
            And | Or => OpCode::Add,
        },
    }
}

pub fn lower(
    module: &HirModule,
    source_file: Rc<str>,
    export_names: Vec<Rc<str>>,
) -> Result<FunctionProto, OptError> {
    crate::ssa::lower_module(module, source_file, export_names)
}
