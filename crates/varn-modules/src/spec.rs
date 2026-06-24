pub const CORE_PREFIX: &str = "core:";
pub const STD_PREFIX: &str = "std:";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleKind {
    Core,
    Stdlib,
    Runtime,
}

pub struct ModuleSpec {
    pub id: &'static str,
    pub kind: ModuleKind,
    pub vn_source: &'static str,
    pub embedded: Option<&'static str>,
    pub exports: &'static [&'static str],
    pub pure: bool,
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
        }
    }

    pub const fn with_source(mut self, src: &'static str) -> Self {
        self.embedded = Some(src);
        self
    }

    pub const fn with_exports(mut self, exports: &'static [&'static str]) -> Self {
        self.exports = exports;
        self
    }

    pub const fn pure_module(mut self) -> Self {
        self.pure = true;
        self
    }

    pub fn source(&self) -> Option<&'static str> {
        self.embedded
    }
}
