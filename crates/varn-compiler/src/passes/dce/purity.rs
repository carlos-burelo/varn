use crate::hir::{HirBinOp, HirUnOp};
use crate::ssa::ir::InstKind;
use varn_tir::BackendTy;

pub(crate) fn dest_droppable(kind: &InstKind) -> bool {
    use InstKind::*;
    matches!(
        kind,
        SelfCall { .. }
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

        GetFixedField { .. } | ArrayGetIndex { .. } | BytesGetIndex { .. } | MapGetIndex { .. } => {
            true
        }

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
            let typed = matches!(ty, BackendTy::Int | BackendTy::Float | BackendTy::Bool);
            let int_can_overflow = *ty == BackendTy::Int
                && matches!(
                    op,
                    HirBinOp::Add
                        | HirBinOp::Sub
                        | HirBinOp::Mul
                        | HirBinOp::Shl
                        | HirBinOp::Shr
                        | HirBinOp::Ushr
                );

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
            HirUnOp::Neg => matches!(ty, BackendTy::Float),
            HirUnOp::Not | HirUnOp::BitNot => {
                matches!(ty, BackendTy::Int | BackendTy::Float | BackendTy::Bool)
            }
        },

        GetProperty { .. } | GetPropertyMaybe { .. } | GetIndex { .. } | GetSuper { .. } => false,

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
        | BytesSetIndex { .. }
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
