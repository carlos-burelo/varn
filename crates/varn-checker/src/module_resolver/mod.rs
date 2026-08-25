pub mod cache;
pub mod exports;
pub mod graph;
pub mod paths;
pub mod resolver;
pub mod stdlib;

pub use cache::{deserialize_module_interface, serialize_module_interface, ExportMap};
pub use graph::ModuleGraph;
pub use paths::{is_known_module, resolve_package_specifier_path};
pub use resolver::{DiskResolver, ImportResolver};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binder::Binder;
    use rustc_hash::FxHashMap;

    fn empty_bind_result() -> BindResult {
        let (tokens, lexeme_buf, _lex_errs) = varn_lexer::scan("", "<test>");
        let program = varn_parser::parse(tokens, lexeme_buf, "<test>").unwrap();
        Binder::bind(&program)
    }

    #[test]
    fn interface_blob_roundtrip() {
        let exports: ExportMap = FxHashMap::default();
        let bind = empty_bind_result();
        let bytes = serialize_module_interface(&exports, &bind).unwrap();
        let (e2, _b2) = deserialize_module_interface(&bytes).unwrap();
        assert_eq!(e2.len(), 0);
    }
}
