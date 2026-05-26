pub const CORE_PREFIX: &str = "core:";
pub const STD_PREFIX: &str = "std:";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleKind {
    /// Intrinsic domain: primitive methods, compiler-known symbols.
    Core,

    /// Public standard library: stable API, versioned.
    Stdlib,

    /// ABI boundary: low-level runtime ops, native-only.
    Runtime,
}

pub struct ModuleSpec {
    pub id: &'static str,
    pub kind: ModuleKind,

    pub vn_source: &'static str,

    pub embedded: Option<&'static str>,
}

impl ModuleSpec {
    pub const fn new(id: &'static str, kind: ModuleKind, vn_source: &'static str) -> Self {
        Self {
            id,
            kind,
            vn_source,
            embedded: None,
        }
    }

    pub const fn with_source(mut self, src: &'static str) -> Self {
        self.embedded = Some(src);
        self
    }

    pub fn source(&self) -> Option<&'static str> {
        self.embedded
    }
}
