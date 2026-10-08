use varn_core::OpCode;

pub(super) fn infix(op: OpCode) -> Option<&'static str> {
    use OpCode as O;
    Some(match op {
        O::Add | O::AddInt | O::AddFloat | O::AddImm | O::StrConcat => "+",
        O::Sub | O::SubInt | O::SubFloat | O::SubImm => "-",
        O::Mul | O::MulInt | O::MulFloat => "*",
        O::Div | O::DivInt | O::DivFloat => "/",
        O::Mod | O::ModInt | O::ModFloat => "%",
        O::Pow | O::PowInt | O::PowFloat => "**",
        O::BitAnd => "&",
        O::BitOr => "|",
        O::BitXor => "^",
        O::Shl => "<<",
        O::Shr => ">>",
        O::Ushr => ">>>",
        O::Eq | O::EqInt | O::EqFloat => "==",
        O::Neq | O::NeqInt | O::NeqFloat => "!=",
        O::Lt | O::LtInt | O::LtFloat => "<",
        O::Lte | O::LteInt | O::LteFloat => "<=",
        O::Gt | O::GtInt | O::GtFloat => ">",
        O::Gte | O::GteInt | O::GteFloat => ">=",
        O::In => "in",
        O::Instanceof => "instanceof",
        O::LoadConst
        | O::LoadNull
        | O::LoadTrue
        | O::LoadFalse
        | O::LoadInt
        | O::Move
        | O::LoadGlobal
        | O::StoreGlobal
        | O::DefineGlobal
        | O::DefineGlobalIdx
        | O::LoadGlobalIdx
        | O::StoreGlobalIdx
        | O::LoadUpvalue
        | O::StoreUpvalue
        | O::CloseUpvalue
        | O::Negate
        | O::Not
        | O::ToString
        | O::Jump
        | O::JumpIfFalse
        | O::JumpIfTrue
        | O::Loop
        | O::Call
        | O::CallMethod
        | O::InvokeVirtual
        | O::CallSpread
        | O::Return
        | O::BuildArray
        | O::BuildTuple
        | O::BuildObject
        | O::BuildObjectWithShape
        | O::BuildRecord
        | O::GetIndex
        | O::SetIndex
        | O::ObjectRest
        | O::ObjectKeys
        | O::ObjectMerge
        | O::GetProperty
        | O::GetPropertyMaybe
        | O::SetProperty
        | O::GetFixedField
        | O::SetFixedField
        | O::GetSuper
        | O::GetSymbol
        | O::MakeClosure
        | O::MakeClass
        | O::Inherit
        | O::Method
        | O::DefineStatic
        | O::DefineGetter
        | O::DefineSetter
        | O::DefineStaticGetter
        | O::DefineStaticSetter
        | O::DeclareLayout
        | O::AllocInstance
        | O::BindMethod
        | O::Typeof
        | O::IsNull
        | O::IsArray
        | O::AssertNotNull
        | O::StrLength
        | O::StrSlice
        | O::ArrayLength
        | O::ArrayPush
        | O::ArrayPop
        | O::ArrayExtend
        | O::WrapSpread
        | O::MakeEnumVariant
        | O::GetEnumTag
        | O::Await
        | O::Spawn
        | O::Yield
        | O::Try
        | O::Throw
        | O::PopTry
        | O::LoadModule
        | O::LoadModuleSlot
        | O::StoreModuleSlot
        | O::InvokeRuntimeStatic
        | O::BuildStr
        | O::LoadIntZero
        | O::LoadIntOne
        | O::LoadIntMinusOne
        | O::Intrinsic
        | O::LoadStaticFn
        | O::CallSelf
        | O::Nop
        | O::ArrayGetIndex
        | O::ArraySetIndex
        | O::CallNativeOp
        | O::IntrinsicDirect
        | O::LoadNativeGlobalIdx
        | O::BuildMap
        | O::MapGetIndex
        | O::MapSetIndex
        | O::Convert
        | O::BytesLength => return None,
    })
}
