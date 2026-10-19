use std::fs;
use std::io::Write;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/modules");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir).join("registry.generated.rs");
    let mut out = fs::File::create(&out_path).expect("failed to create registry.generated.rs");

    writeln!(out, "pub static MODULE_REGISTRY: &[ModuleSpec] = &[").unwrap();
    for (layer, kind) in [
        ("core", "ModuleKind::Core"),
        ("runtime", "ModuleKind::Runtime"),
    ] {
        let root = Path::new("src/modules").join(layer);
        collect_modules(&root, &root, layer, kind, &mut out);
    }
    writeln!(out, "];").unwrap();
}

fn collect_modules(root: &Path, dir: &Path, layer: &str, kind: &str, out: &mut impl Write) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();

    let contracts: Vec<&Path> = paths
        .iter()
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("vn"))
        .map(|p| p.as_path())
        .collect();
    match contracts.as_slice() {
        [] => {}
        [contract] => {
            let rel = dir
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            assert!(
                !rel.is_empty(),
                "{} is a layer root, not a module",
                dir.display()
            );
            emit_spec_entry(out, &format!("{layer}:{rel}"), kind, contract);
        }
        many => panic!(
            "{} holds {} .vn contracts; a module is exactly one",
            dir.display(),
            many.len()
        ),
    }

    for sub in paths.iter().filter(|p| p.is_dir()) {
        collect_modules(root, sub, layer, kind, out);
    }
}

fn emit_spec_entry(out: &mut impl Write, id: &str, kind_expr: &str, contract: &Path) {
    let include_path = contract.to_string_lossy().replace('\\', "/");
    let source =
        fs::read_to_string(contract).unwrap_or_else(|e| panic!("cannot read {include_path}: {e}"));
    let surface = ContractSurface::parse(&source, &include_path);
    let exports: String = surface
        .exports
        .iter()
        .map(|e| format!(r#""{e}","#))
        .collect();
    let code = if surface.has_code { ".with_code()" } else { "" };
    writeln!(
        out,
        r#"    ModuleSpec::new("{id}", {kind_expr}, "crates/varn-builtins/{include_path}").with_source(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/{include_path}"))).with_exports(&[{exports}]){code},"#,
    )
    .unwrap();
}

struct ContractSurface {
    exports: Vec<String>,
    has_code: bool,
}

impl ContractSurface {
    fn parse(source: &str, name: &str) -> Self {
        use varn_core::ast::{Decl, ExportDecl, StmtKind};
        let (tokens, lexeme_buf, _) = varn_lexer::scan(source, name);
        let (program, interner, arena) =
            varn_parser::parse(tokens, lexeme_buf, name, varn_core::AtomInterner::new())
                .unwrap_or_else(|_| panic!("cannot parse contract {name}"));
        let mut exports = Vec::new();
        let mut has_code = false;
        for &stmt in &program.body {
            let StmtKind::Decl(decl) = &arena.stmt(stmt).kind else {
                continue;
            };
            let Decl::Export(export) = decl.as_ref() else {
                continue;
            };
            let ExportDecl::Decl { declaration, .. } = export else {
                has_code = true;
                continue;
            };
            let name = |a: varn_core::Atom| interner.resolve(a).to_string();
            let (names, declared): (Vec<String>, bool) = match declaration.as_ref() {
                Decl::Function(f) => (vec![name(f.id)], f.modifiers.is_declare),
                Decl::Class(c) => (c.id.map(name).into_iter().collect(), c.modifiers.is_declare),
                Decl::Interface(i) => (vec![name(i.id)], true),
                Decl::TypeAlias(t) => (vec![name(t.id)], true),
                Decl::Namespace(n) => (vec![name(n.id)], n.is_declare),
                Decl::Enum(e) => (vec![name(e.id)], e.is_declare),
                Decl::Struct(st) => (vec![name(st.id)], false),
                Decl::SumType(st) => (vec![name(st.id)], false),
                Decl::Variable(v) => (
                    v.declarators
                        .iter()
                        .filter_map(|d| match &d.id {
                            varn_core::ast::Pattern::Identifier { name: n, .. } => Some(name(*n)),
                            varn_core::ast::Pattern::Array { .. }
                            | varn_core::ast::Pattern::Object { .. }
                            | varn_core::ast::Pattern::Assignment { .. }
                            | varn_core::ast::Pattern::Rest { .. } => None,
                        })
                        .collect(),
                    v.is_declare,
                ),
                Decl::Import(_) | Decl::Export(_) | Decl::Extension(_) => (Vec::new(), false),
            };
            exports.extend(names);
            has_code |= !declared;
        }
        exports.sort();
        exports.dedup();
        ContractSurface { exports, has_code }
    }
}
