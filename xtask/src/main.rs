//! Repo tooling. `cargo xtask build-std` compiles the std/ tree into a
//! versioned std.vnb bundle (spec: docs/superpowers/specs/
//! 2026-07-09-stdlib-package-system-design.md §6).

use std::path::PathBuf;

use varn_modules::artifact::BUILD_FINGERPRINT;
use varn_modules::bundle::{write_bundle, BundleModule, StdBundle};

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("build-std") => build_std(args.collect()),
        other => {
            eprintln!("usage: cargo xtask build-std [--std-dir <dir>] [--out <file>]");
            if other.is_some() {
                std::process::exit(2);
            }
        }
    }
}

fn build_std(args: Vec<String>) {
    let mut std_dir = PathBuf::from("std");
    let mut out = PathBuf::from("std.vnb");
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--std-dir" => std_dir = PathBuf::from(it.next().expect("--std-dir needs a value")),
            "--out" => out = PathBuf::from(it.next().expect("--out needs a value")),
            other => {
                eprintln!("unknown arg: {other}");
                std::process::exit(2);
            }
        }
    }

    // Serve std: resolution from the tree we are building.
    std::env::set_var(varn_modules::std_root::ENV_VARN_STD, &std_dir);
    varn_builtins::register_provider();

    let manifest_raw = std::fs::read_to_string(std_dir.join("std.json"))
        .unwrap_or_else(|e| panic!("cannot read {}/std.json: {e}", std_dir.display()));
    #[derive(serde::Deserialize)]
    struct Manifest {
        version: String,
        #[serde(rename = "hostApi")]
        host_api: u32,
        modules: Vec<ManifestModule>,
    }
    #[derive(serde::Deserialize)]
    struct ManifestModule {
        id: String,
        #[serde(default)]
        pure: bool,
    }
    let manifest: Manifest = serde_json::from_str(&manifest_raw).expect("invalid std.json");
    assert_eq!(
        manifest.host_api,
        varn_core::HOST_API_VERSION,
        "std.json hostApi does not match this binary's HOST_API_VERSION"
    );

    let mut modules = Vec::new();
    for m in &manifest.modules {
        let file = std_dir.join(format!("{}.vn", m.id.strip_prefix("std:").expect("std: id")));
        let source = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));

        validate_imports(&m.id, &source);

        let exports = varn_checker::module_resolver::resolve_stdlib_module_exports_ref(&m.id);
        let bind = varn_checker::module_resolver::resolve_stdlib_module_bind_ref(&m.id)
            .unwrap_or_else(|| panic!("cannot bind {}", m.id));
        let interface =
            varn_checker::module_resolver::serialize_module_interface(&exports, &bind)
                .unwrap_or_else(|e| panic!("interface serialization failed for {}: {e}", m.id));

        let proto = varn_pipeline::stdlib_loader::compile_source(&source, &m.id)
            .unwrap_or_else(|e| panic!("compile error in {}: {e}", m.id));
        let bytecode = postcard::to_allocvec(&proto)
            .unwrap_or_else(|e| panic!("bytecode serialization failed for {}: {e}", m.id));

        println!(
            "  {} — interface {} B, bytecode {} B",
            m.id,
            interface.len(),
            bytecode.len()
        );
        modules.push(BundleModule {
            id: m.id.clone(),
            pure: m.pure,
            interface,
            bytecode,
        });
    }

    let bundle = StdBundle {
        std_version: manifest.version.clone(),
        build_fingerprint: BUILD_FINGERPRINT,
        host_api_version: varn_core::HOST_API_VERSION,
        modules,
    };
    let bytes = write_bundle(&bundle);
    std::fs::write(&out, &bytes).unwrap_or_else(|e| panic!("cannot write {}: {e}", out.display()));
    println!(
        "std v{} → {} ({} modules, {} KiB)",
        manifest.version,
        out.display(),
        manifest.modules.len(),
        bytes.len() / 1024
    );
}

/// Spec §2: std modules may only import runtime:* and std:*.
///
/// Parser-based (not string-scanning): lex + parse the module, then reuse
/// the same import collector the pipeline uses for cache invalidation.
/// This avoids false positives/negatives from string literals that merely
/// resemble import syntax.
fn validate_imports(id: &str, source: &str) {
    let (tokens, lexeme_buf, _) = varn_lexer::scan(source, id);
    let program = match varn_parser::parse(tokens, lexeme_buf, id) {
        Ok(p) => p,
        Err(errs) => panic!("{id}: parse error: {}", errs[0].message),
    };
    for spec in varn_pipeline::import_collector::collect_imports(&program) {
        if !(spec.starts_with("runtime:") || spec.starts_with("std:")) {
            panic!("{id}: forbidden import \"{spec}\" — std may only import runtime:*/std:*");
        }
    }
}
