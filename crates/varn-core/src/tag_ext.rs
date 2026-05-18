use varn_base::TypeTag;

pub trait TypeTagExt {
    fn to_intrinsic_str(self) -> &'static str;
    fn to_runtime_str(self) -> &'static str;
    fn is_primitive(self) -> bool;
}

impl TypeTagExt for TypeTag {
    fn to_intrinsic_str(self) -> &'static str {
        match self {
            TypeTag::Function => "function",
            TypeTag::Array => "Array",
            TypeTag::Map => "Map",
            TypeTag::Set => "Set",
            TypeTag::Task => "Task",
            TypeTag::Generator => "Generator",
            TypeTag::Range => "Range",
            _ => self.name(),
        }
    }

    fn to_runtime_str(self) -> &'static str {
        match self {
            TypeTag::Array => "array",
            TypeTag::Object => "object",
            TypeTag::Function | TypeTag::NativeFn => "fn",
            TypeTag::Class => "class",
            TypeTag::Generator => "generator",
            TypeTag::AsyncQueue => "asyncqueue",
            TypeTag::Enum => "enum",
            _ => self.name(),
        }
    }

    fn is_primitive(self) -> bool {
        use varn_base::TypeFlags;
        (self.flags() & TypeFlags::IS_SCALAR).bits() != 0
    }
}
