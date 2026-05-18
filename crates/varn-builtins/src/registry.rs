use varn_modules::spec::{ModuleKind, ModuleSpec};
use varn_modules::{
    BUILTIN_ARRAY, BUILTIN_ASYNC, BUILTIN_BOOL, BUILTIN_CHAR, BUILTIN_CLASSES, BUILTIN_COLLECTIONS,
    BUILTIN_DECIMAL, BUILTIN_ERRORS, BUILTIN_FLOAT, BUILTIN_GLOBAL, BUILTIN_INT, BUILTIN_ITERATORS,
    BUILTIN_RANGE, BUILTIN_REFLECT, BUILTIN_STR, STD_COLLECTIONS, STD_CORE, STD_CRYPTO,
    STD_DISPOSE, STD_FS, STD_HTTP, STD_IO, STD_JSON, STD_MATH, STD_NET, STD_OPTION, STD_PATH,
    STD_REFLECT, STD_RESULT, STD_SYS, STD_TASK, STD_TEST, STD_TIME, STD_TYPES,
};

macro_rules! primitive_spec {
    ($id:expr, $name:literal) => {
        ModuleSpec::new(
            $id,
            ModuleKind::Builtin,
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
    global_spec!(BUILTIN_GLOBAL, ModuleKind::Builtin, "globals", "globals"),
    global_spec!(BUILTIN_CLASSES, ModuleKind::Builtin, "globals", "classes"),
    global_spec!(BUILTIN_ERRORS, ModuleKind::Builtin, "globals", "errors"),
    global_spec!(
        BUILTIN_COLLECTIONS,
        ModuleKind::Builtin,
        "globals",
        "collections"
    ),
    global_spec!(BUILTIN_ASYNC, ModuleKind::Builtin, "globals", "async"),
    global_spec!(
        BUILTIN_ITERATORS,
        ModuleKind::Builtin,
        "globals",
        "iterators"
    ),
    global_spec!(BUILTIN_REFLECT, ModuleKind::Builtin, "globals", "reflect"),
    primitive_spec!(BUILTIN_STR, "str"),
    primitive_spec!(BUILTIN_INT, "int"),
    primitive_spec!(BUILTIN_FLOAT, "float"),
    primitive_spec!(BUILTIN_BOOL, "bool"),
    primitive_spec!(BUILTIN_CHAR, "char"),
    primitive_spec!(BUILTIN_DECIMAL, "decimal"),
    primitive_spec!(BUILTIN_RANGE, "range"),
    primitive_spec!(BUILTIN_ARRAY, "array"),
    stdlib_spec!(STD_TASK, ModuleKind::Stdlib, "std", "task"),
    stdlib_spec!(STD_COLLECTIONS, ModuleKind::Stdlib, "std", "collections"),
    stdlib_spec!(STD_CRYPTO, ModuleKind::Stdlib, "std", "crypto"),
    global_spec!(STD_DISPOSE, ModuleKind::Stdlib, "globals", "dispose"),
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
    stdlib_spec!(STD_CORE, ModuleKind::Stdlib, "std", "core"),
];

pub fn is_known(specifier: &str) -> bool {
    MODULE_REGISTRY.iter().any(|m| m.id == specifier)
}

pub fn spec_for(specifier: &str) -> Option<&'static ModuleSpec> {
    MODULE_REGISTRY.iter().find(|m| m.id == specifier)
}
