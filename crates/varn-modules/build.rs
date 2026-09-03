use std::io::Write;
use std::path::Path;

/// Crates whose type definitions shape serialized binary artifact schemas.
/// The list must cover every type that reaches a payload, transitively — a
/// gap here is not a stale artifact but a WRONG one: postcard carries no
/// schema, so a struct that gained a field is read back as whatever the new
/// layout says those bytes mean.
///
/// * `varn-types`  — `ModuleGraphArtifact`, `FunctionProto`.
/// * `varn-modules` — `StdBundle` and this envelope.
/// * `varn-checker` — `CachedModule { ExportMap, BindResult }`, which is the
///   payload of every `.vnm` and rides inside the std bundle. It was missing:
///   the bundle is a DISTRIBUTABLE, validated on schema alone, so a checker
///   type change left older bundles looking valid. Nothing broke only because
///   `varn-cli`'s build script rebuilds the bundle whenever `varn-pipeline`
///   recompiles — an accident of the dependency graph, not a guarantee.
///
/// Watching a crate that others depend on does widen recompilation. Measured
/// on this host, touching `varn-checker/src/symbol.rs` and rebuilding
/// `vn --release`: 111 s before, 129 s after — +16%, and only on edits to
/// this one crate. The frontend, compiler, VM and CLI crates stay independent
/// of each other; only the three schema crates pull the graph. That is the
/// price of the guarantee the doc comment above already claimed.
const FINGERPRINTED_CRATES: &[&str] = &["varn-types", "varn-modules", "varn-checker"];

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let crates_root = Path::new(&manifest_dir)
        .parent()
        .expect("varn-modules lives under crates/")
        .to_path_buf();

    let mut hash: u64 = 0xcbf29ce484222325;
    for krate in FINGERPRINTED_CRATES {
        let src = crates_root.join(krate).join("src");
        println!("cargo:rerun-if-changed={}", src.display());
        hash_dir(&src, &mut hash);
    }

    let folded = (hash ^ (hash >> 32)) as u32;
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("build_fingerprint.rs");
    let mut f = std::fs::File::create(&out).expect("cannot create build_fingerprint.rs");
    writeln!(f, "pub const BUILD_FINGERPRINT: u32 = {folded:#010x};").unwrap();
}

fn hash_dir(dir: &Path, hash: &mut u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            hash_dir(&p, hash);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            fnv1a(p.file_name().unwrap().to_string_lossy().as_bytes(), hash);
            if let Ok(bytes) = std::fs::read(&p) {
                fnv1a(&bytes, hash);
            }
        }
    }
}

fn fnv1a(bytes: &[u8], hash: &mut u64) {
    for &b in bytes {
        *hash ^= b as u64;
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}
