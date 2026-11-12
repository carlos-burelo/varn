#![allow(clippy::disallowed_methods)]
use std::io::Write;
use std::path::Path;

const CRATES_WITHOUT_ARTIFACT_OUTPUT: &[&str] = &[
    "vn-cli",
    "vn-dap",
    "vn-debug",
    "vn-gpu",
    "vn-inspect",
    "vn-jit",
    "vn-jit-abi",
    "vn-lsp",
    "vn-pm",
    "vn-project",
    "vn-shadow",
    "vn-text",
    "vn-ui",
    "vn-vm",
    "xtask",
];

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let crates_root = Path::new(&manifest_dir)
        .parent()
        .expect("vn-modules lives under crates/")
        .to_path_buf();

    let mut hash: u64 = 0xcbf29ce484222325;
    let mut crates: Vec<_> = std::fs::read_dir(&crates_root)
        .expect("crates/ is readable")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("src").is_dir())
        .collect();
    crates.sort();
    for krate in crates {
        let name = krate.file_name().unwrap().to_string_lossy().into_owned();
        if CRATES_WITHOUT_ARTIFACT_OUTPUT.contains(&name.as_str()) {
            continue;
        }
        let src = krate.join("src");
        println!("cargo:rerun-if-changed={}", src.display());
        fnv1a(name.as_bytes(), &mut hash);
        hash_dir(&src, &mut hash);
    }

    let lockfile = crates_root
        .parent()
        .expect("crates/ lives under the workspace root")
        .join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lockfile.display());
    if let Ok(bytes) = std::fs::read(&lockfile) {
        fnv1a(&bytes, &mut hash);
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
        } else if matches!(p.extension().and_then(|e| e.to_str()), Some("rs" | "vn")) {
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
