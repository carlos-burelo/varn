macro_rules! literals_vars_ops {
    () => {
        OpCode::LoadGlobal
            | OpCode::StoreGlobal
            | OpCode::DefineGlobal
            | OpCode::LoadUpvalue
            | OpCode::StoreUpvalue
            | OpCode::CloseUpvalue
            | OpCode::LoadTrue
            | OpCode::LoadFalse
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne
    };
}
pub(super) use literals_vars_ops;

macro_rules! math_cmp_ops {
    () => {
        OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::Ushr
            | OpCode::Negate
            | OpCode::Not
            | OpCode::AddImm
            | OpCode::SubImm
            | OpCode::AddInt
            | OpCode::SubInt
            | OpCode::MulInt
            | OpCode::DivInt
            | OpCode::ModInt
            | OpCode::PowInt
            | OpCode::LtInt
            | OpCode::GtInt
            | OpCode::LteInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt
            | OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::ModFloat
            | OpCode::PowFloat
            | OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::EqFloat
            | OpCode::NeqFloat
            | OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::ToString
            | OpCode::StrConcat
            | OpCode::BuildStr
            | OpCode::StrLength
            | OpCode::StrSlice
    };
}
pub(super) use math_cmp_ops;

macro_rules! control_call_ops {
    () => {
        OpCode::Jump
            | OpCode::Loop
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue
            | OpCode::Return
            | OpCode::Call
            | OpCode::CallSelf
            | OpCode::CallMethod
            | OpCode::InvokeVirtual
            | OpCode::CallSpread
    };
}
pub(super) use control_call_ops;

macro_rules! object_ops {
    () => {
        OpCode::MakeClosure
            | OpCode::GetProperty
            | OpCode::GetPropertyMaybe
            | OpCode::SetProperty
            | OpCode::GetFixedField
            | OpCode::SetFixedField
            | OpCode::GetSuper
            | OpCode::GetSymbol
            | OpCode::AssertNotNull
            | OpCode::DeclareField
            | OpCode::GetIndex
            | OpCode::SetIndex
            | OpCode::ArrayGetIndex
            | OpCode::ArraySetIndex
            | OpCode::BuildArray
            | OpCode::BuildTuple
            | OpCode::BuildObject
            | OpCode::BuildObjectWithShape
            | OpCode::BuildRecord
            | OpCode::ObjectRest
            | OpCode::ObjectKeys
            | OpCode::ObjectMerge
            | OpCode::WrapSpread
            | OpCode::ArrayLength
            | OpCode::BytesLength
            | OpCode::ArrayPush
            | OpCode::ArrayPop
            | OpCode::ArrayExtend
            | OpCode::In
            | OpCode::Instanceof
            | OpCode::Typeof
            | OpCode::IsNull
            | OpCode::IsArray
            | OpCode::BuildMap
            | OpCode::MapGetIndex
            | OpCode::MapSetIndex
    };
}
pub(super) use object_ops;

macro_rules! class_ops {
    () => {
        OpCode::MakeClass
            | OpCode::Inherit
            | OpCode::Method
            | OpCode::DefineStatic
            | OpCode::DefineGetter
            | OpCode::DefineSetter
            | OpCode::DefineStaticGetter
            | OpCode::DefineStaticSetter
            | OpCode::BindMethod
    };
}
pub(super) use class_ops;
