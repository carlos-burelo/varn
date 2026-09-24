pub const CORE_PREFIX: &str = "core:";
pub const STD_PREFIX: &str = "std:";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleKind {
    Core,
    Stdlib,
    Runtime,
}

#[derive(Clone, Copy, Debug)]
pub struct ModuleSpec {
    pub id: &'static str,
    pub kind: ModuleKind,
    pub vn_source: &'static str,
    pub embedded: Option<&'static str>,
    pub exports: &'static [&'static str],
    pub pure: bool,
    /// A core module holding Varn code (not only native declarations): it is
    /// compiled and evaluated, and the prelude links its exports into every
    /// module (ADR-0018).
    pub has_code: bool,
}

impl ModuleSpec {
    pub const fn new(id: &'static str, kind: ModuleKind, vn_source: &'static str) -> Self {
        Self {
            id,
            kind,
            vn_source,
            embedded: None,
            exports: &[],
            pure: false,
            has_code: false,
        }
    }

    pub const fn with_source(mut self, src: &'static str) -> Self {
        self.embedded = Some(src);
        self
    }

    pub const fn with_code(mut self) -> Self {
        self.has_code = true;
        self
    }

    pub const fn with_exports(mut self, exports: &'static [&'static str]) -> Self {
        self.exports = exports;
        self
    }

    pub fn source(&self) -> Option<&'static str> {
        self.embedded
    }

    /// Materialize a spec whose strings live for the process lifetime.
    /// Used for std modules loaded from a bundle/tree at startup; the std
    /// set is small and lives as long as the VM, so leaking is correct.
    pub fn leaked(id: String, kind: ModuleKind, vn_source: String, pure: bool) -> Self {
        Self {
            id: Box::leak(id.into_boxed_str()),
            kind,
            vn_source: Box::leak(vn_source.into_boxed_str()),
            embedded: None,
            exports: &[],
            pure,
            has_code: false,
        }
    }
}
