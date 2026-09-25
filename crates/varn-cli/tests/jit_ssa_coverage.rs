//! Every function the suite compiles is lowered from typed SSA.
//!
//! The bytecode lowering is still there as a fallback, so a function that
//! loses its portable SSA — a new instruction without a portable form, a
//! merge whose values disagree in representation — keeps passing every
//! behavioural test while silently leaving the SSA lowering. This runs
//! `tests/main.vn` with the lowering traced and fails on any function that
//! declined (`from_ssa unavailable`) or bailed (`from_ssa bail`), by name and
//! reason.

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
fn every_compiled_function_lowers_from_ssa() {
    let root = repo_root();
    let cache = std::env::temp_dir().join(format!("varn-ssa-cov-{}", std::process::id()));
    std::fs::create_dir_all(&cache).expect("cache dir");
    let out = Command::new(env!("CARGO_BIN_EXE_vn"))
        .arg("run")
        .arg(root.join("tests/main.vn"))
        .current_dir(&root)
        .env("VARN_CACHE_DIR", &cache)
        .env("NO_COLOR", "1")
        .env("VARN_CLIF_TRACE", "1")
        .output()
        .expect("spawn vn");
    let _ = std::fs::remove_dir_all(&cache);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stdout.contains("ALL TESTS PASSED"),
        "suite failed:\n{stdout}\n{stderr}"
    );
    let off_ssa: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("from_ssa unavailable") || l.contains("from_ssa bail"))
        .collect();
    assert!(
        off_ssa.is_empty(),
        "functions left the SSA lowering:\n{}",
        off_ssa.join("\n")
    );
}
