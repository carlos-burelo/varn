//! On-stack replacement from typed SSA, forced.
//!
//! A function with a loop is compiled at its first call, so the ordinary
//! suite almost never resumes a running frame mid-loop. This runs
//! `tests/151-jit-osr.vn` with the entry thresholds out of reach and the OSR
//! threshold low, so every function in it reaches compiled code only through
//! an OSR entry — and checks both the results and that the entry taken was
//! the one lowered from SSA.

// Un test de integración no usa las dependencias de la librería.
#![allow(unused_crate_dependencies)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

#[test]
fn osr_entries_resume_from_ssa() {
    let root = repo_root();
    let cache = tempfile_dir();
    let out = Command::new(env!("CARGO_BIN_EXE_vn"))
        .arg("run")
        .arg(root.join("tests/151-jit-osr.vn"))
        .env("VARN_CACHE_DIR", &cache)
        .env("NO_COLOR", "1")
        .env("VARN_JIT_TIER", "100000000")
        .env("VARN_JIT_TIER_STRAIGHT", "100000000")
        .env("VARN_JIT_OSR", "50")
        .env("VARN_CLIF_TRACE", "1")
        .output()
        .expect("spawn vn");
    let _ = std::fs::remove_dir_all(&cache);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stdout.contains("[PASSED] 151."),
        "OSR run failed:\n{stdout}\n{stderr}"
    );
    for name in ["osSingle", "osNested", "osTriple", "osDown"] {
        assert!(
            stderr
                .lines()
                .any(|l| l.contains(&format!("from_ssa {name} osr@"))),
            "{name} did not resume through an SSA OSR entry:\n{stderr}"
        );
    }
}

/// A fresh cache directory, so the run compiles from source.
fn tempfile_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("varn-osr-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("cache dir");
    dir
}
