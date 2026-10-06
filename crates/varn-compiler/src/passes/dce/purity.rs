use crate::hir::{HirBinOp, HirType, HirUnOp};
use crate::ssa::ir::InstKind;

pub(crate) fn dest_droppable(kind: &InstKind) -> bool {
    use InstKind::*;
    matches!(
        kind,
        Call { .. }
            | SelfCall { .. }
            | MethodCall { .. }
            | SuperCall { .. }
            | SuperMethodCall { .. }
            | ExtensionCall { .. }
            | IntrinsicCall { .. }
            | CallNativeOp { .. }
    )
}

pub(crate) fn is_pure(kind: &InstKind) -> bool {
    use InstKind::*;
    match kind {
        ConstInt(_)
        | ConstFloat(_)
        | ConstBool(_)
        | ConstStr(_)
        | ConstChar(_)
        | ConstDecimal(_)
        | ConstBigInt(_)
        | ConstNull
        | LoadGlobal(_)
        | LoadGlobalIdx(_)
        | LoadNativeGlobalIdx(_)
        | LoadUpvalue(_)
        | LoadCaptured { .. }
        | ModuleSlot { .. }
        | This
        | CatchParam { .. } => true,

        GetFixedField { .. } | ArrayGetIndex { .. } | MapGetIndex { .. } => true,

        IsNull { .. } | Cast { .. } | IsArray { .. } | GetEnumTag { .. } | ObjectKeys { .. } => {
            true
        }

        StrLength { .. } | ArrayLength { .. } | BytesLength { .. } => true,
        Convert { conv, .. } => !conv.can_fault(),

        AllocInstance { .. }
        | BuildArray { .. }
        | BuildTuple { .. }
        | BuildObject { .. }
        | BuildRecord { .. }
        | BuildMap { .. }
        | MakeClosure { .. }
        | MakeEnumVariant { .. }
        | Range { .. } => true,

        Binary { op, ty, .. } => {
            let typed = matches!(ty, HirType::Int | HirType::Float | HirType::Bool);
            let int_can_overflow =
                *ty == HirType::Int && matches!(op, HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul);

            let never_traps = matches!(
                op,
                HirBinOp::Add
                    | HirBinOp::Sub
                    | HirBinOp::Mul
                    | HirBinOp::Eq
                    | HirBinOp::Ne
                    | HirBinOp::Lt
                    | HirBinOp::Le
                    | HirBinOp::Gt
                    | HirBinOp::Ge
                    | HirBinOp::BitAnd
                    | HirBinOp::BitOr
                    | HirBinOp::BitXor
                    | HirBinOp::Shl
                    | HirBinOp::Shr
                    | HirBinOp::Ushr
            );
            typed && never_traps && !int_can_overflow
        }
        Unary { op, ty, .. } => match op {
            HirUnOp::Typeof => true,
            HirUnOp::Neg => matches!(ty, HirType::Float),
            HirUnOp::Not | HirUnOp::BitNot => {
                matches!(ty, HirType::Int | HirType::Float | HirType::Bool)
            }
        },

        GetProperty { .. }
        | GetPropertyMaybe { .. }
        | GetIndex { .. }
        | GetSuper { .. }
        | GetSymbol { .. } => false,

        ToString { .. } | BuildStr { .. } => false,

        BuildArraySpread { .. }
        | BuildObjectSpread { .. }
        | CallSpread { .. }
        | ObjectRest { .. } => false,

        Call { .. }
        | SelfCall { .. }
        | MethodCall { .. }
        | SuperCall { .. }
        | SuperMethodCall { .. }
        | ExtensionCall { .. }
        | IterCall { .. }
        | IntrinsicCall { .. }
        | CallNativeOp { .. }
        | ArrayPush { .. } => false,

        StoreGlobal { .. }
        | StoreGlobalIdx { .. }
        | StoreUpvalue { .. }
        | StoreCaptured { .. }
        | StoreModuleSlot { .. }
        | SetProperty { .. }
        | SetFixedField { .. }
        | SetIndex { .. }
        | ArraySetIndex { .. }
        | MapSetIndex { .. }
        | ObjectMerge { .. } => false,

        MakeClass { .. }
        | DeclareLayout { .. }
        | DefineStatic { .. }
        | DefineMethod { .. }
        | DefineAccessor { .. } => false,

        Try { .. }
        | PopTry
        | CloseUpvalues { .. }
        | Dispose { .. }
        | LoadModule { .. }
        | AssertNotNull { .. }
        | Await { .. }
        | Spawn { .. }
        | Yield { .. } => false,
    }
}

#[cfg(test)]
#[path = "purity_tests.rs"]
mod purity_tests;
