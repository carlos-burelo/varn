use varn_modules::spec::{ModuleKind, ModuleSpec};
use varn_modules::{
    CORE_ARRAY, CORE_ASYNC, CORE_BIGINT, CORE_BOOL, CORE_CHAR, CORE_COLLECTIONS, CORE_DECIMAL,
    CORE_FLOAT, CORE_GLOBAL, CORE_INT, CORE_ITERATORS, CORE_MAP, CORE_RANGE, CORE_REFLECT,
    CORE_SET, CORE_STR, CORE_SYMBOL, STD_COLLECTIONS, STD_CRYPTO, STD_DISPOSE, STD_FS, STD_HTTP,
    STD_IO, STD_JSON, STD_MATH, STD_NET, STD_OPTION, STD_PATH, STD_REFLECT, STD_RESULT, STD_SYS,
    STD_TASK, STD_TEST, STD_TIME, STD_TYPES,
};

macro_rules! primitive_spec {
    ($id:expr, $name:literal) => {
        ModuleSpec::new(
            $id,
            ModuleKind::Core,
            concat!(
                "crates/varn-builtins/src/modules/primitives/",
                $name,
                "/",
                $name,
                ".vn"
            ),
        )
        .with_source(include_str!(concat!(
            "modules/primitives/",
            $name,
            "/",
            $name,
            ".vn"
        )))
    };
}

macro_rules! stdlib_spec {
    ($id:expr, $kind:expr, $cat:literal, $name:literal) => {
        ModuleSpec::new(
            $id,
            $kind,
            concat!(
                "crates/varn-builtins/src/modules/",
                $cat,
                "/",
                $name,
                "/",
                $name,
                ".vn"
            ),
        )
        .with_source(include_str!(concat!(
            "modules/", $cat, "/", $name, "/", $name, ".vn"
        )))
    };
}

macro_rules! global_spec {
    ($id:expr, $kind:expr, $cat:literal, $name:literal) => {
        ModuleSpec::new(
            $id,
            $kind,
            concat!("crates/varn-builtins/src/modules/", $cat, "/", $name, ".vn"),
        )
        .with_source(include_str!(concat!("modules/", $cat, "/", $name, ".vn")))
    };
}

pub static MODULE_REGISTRY: &[ModuleSpec] = &[
    global_spec!(CORE_GLOBAL, ModuleKind::Core, "globals", "globals"),
    primitive_spec!(CORE_BIGINT, "bigint"),
    primitive_spec!(CORE_MAP, "map"),
    primitive_spec!(CORE_SET, "set"),
    primitive_spec!(CORE_SYMBOL, "symbol"),
    stdlib_spec!(CORE_COLLECTIONS, ModuleKind::Core, "std", "collections"),
    stdlib_spec!(CORE_ASYNC, ModuleKind::Core, "std", "task"),
    stdlib_spec!(CORE_ITERATORS, ModuleKind::Core, "std", "task"),
    stdlib_spec!(CORE_REFLECT, ModuleKind::Core, "std", "reflect"),
    primitive_spec!(CORE_STR, "str"),
    primitive_spec!(CORE_INT, "int"),
    primitive_spec!(CORE_FLOAT, "float"),
    primitive_spec!(CORE_BOOL, "bool"),
    primitive_spec!(CORE_CHAR, "char"),
    primitive_spec!(CORE_DECIMAL, "decimal"),
    primitive_spec!(CORE_RANGE, "range"),
    primitive_spec!(CORE_ARRAY, "array"),
    stdlib_spec!(STD_TASK, ModuleKind::Stdlib, "std", "task"),
    stdlib_spec!(STD_COLLECTIONS, ModuleKind::Stdlib, "std", "collections"),
    stdlib_spec!(STD_CRYPTO, ModuleKind::Stdlib, "std", "crypto"),
    stdlib_spec!(STD_DISPOSE, ModuleKind::Stdlib, "std", "dispose"),
    stdlib_spec!(STD_FS, ModuleKind::Stdlib, "std", "fs"),
    stdlib_spec!(STD_HTTP, ModuleKind::Stdlib, "std", "http"),
    stdlib_spec!(STD_IO, ModuleKind::Stdlib, "std", "io"),
    stdlib_spec!(STD_JSON, ModuleKind::Stdlib, "std", "json"),
    stdlib_spec!(STD_MATH, ModuleKind::Stdlib, "std", "math"),
    stdlib_spec!(STD_NET, ModuleKind::Stdlib, "std", "net"),
    stdlib_spec!(STD_OPTION, ModuleKind::Stdlib, "std", "option"),
    stdlib_spec!(STD_PATH, ModuleKind::Stdlib, "std", "path"),
    stdlib_spec!(STD_REFLECT, ModuleKind::Stdlib, "std", "reflect"),
    stdlib_spec!(STD_RESULT, ModuleKind::Stdlib, "std", "result"),
    stdlib_spec!(STD_SYS, ModuleKind::Stdlib, "std", "sys"),
    stdlib_spec!(STD_TEST, ModuleKind::Stdlib, "std", "testing"),
    stdlib_spec!(STD_TIME, ModuleKind::Stdlib, "std", "time"),
    stdlib_spec!(STD_TYPES, ModuleKind::Stdlib, "std", "types"),
];

pub fn is_known(specifier: &str) -> bool {
    MODULE_REGISTRY.iter().any(|m| m.id == specifier)
}

pub fn spec_for(specifier: &str) -> Option<&'static ModuleSpec> {
    MODULE_REGISTRY.iter().find(|m| m.id == specifier)
}
