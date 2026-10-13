use crate::checker::Checker;
use varn_core::TypeKind;
use varn_sem::types::{FunctionParam, Type};

impl<'r> Checker<'r> {
    pub(super) fn accepted_param_type(&mut self, param: &FunctionParam, declared: Type) -> Type {
        if param.optional && !param.is_rest && !declared.is_dynamic() {
            Type::make_nullable(declared, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
        } else {
            declared
        }
    }
}

pub(super) fn compatible_param_type(
    param: &FunctionParam,
    spread_inner: Option<Type>,
    table: &varn_sem::types::CheckerTyTable,
) -> Type {
    if let Some(inner) = spread_inner {
        if param.is_rest {
            if let TypeKind::Array(expected_inner) = table.get(param.ty) {
                Type::resolved(expected_inner)
            } else {
                Type::resolved(param.ty)
            }
        } else {
            inner
        }
    } else if param.is_rest {
        if let TypeKind::Array(inner) = table.get(param.ty) {
            Type::resolved(inner)
        } else {
            Type::resolved(param.ty)
        }
    } else {
        Type::resolved(param.ty)
    }
}

pub(super) fn named_slot_problem(
    label: Option<&str>,
    slot: Option<usize>,
    taken: bool,
) -> Option<String> {
    match (label, slot) {
        (Some(label), None) => Some(format!("no parameter is named '{label}'")),
        (Some(label), Some(_)) if taken => {
            Some(format!("the parameter '{label}' is given more than once"))
        }
        (Some(_), Some(_)) | (None, _) => None,
    }
}
