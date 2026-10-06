#![allow(unused_crate_dependencies)]















use std::path::{Path, PathBuf};
use std::process::Command;



const PHASES: &[&str] = &[
    "tokens",
    "ast",
    "symbols",
    "modules",
    "scope",
    "caps",
    "check:types",
    "tir",
    "tir:check",
    "bytecode",
    "summary",
    "typeloss",
    "graph",
];

#[cfg(target_arch = "x86_64")]
const JIT_PHASES: &[&str] = &["tiers", "bails", "clif:route", "clif:kinds"];

const FIXTURES: &[&str] = &["arith", "loop_array", "class_fields", "generics", "closure"];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../varn-pipeline/tests/fixtures/debug")
}

fn golden_name(case: &str, phase: &str) -> String {
    format!("{case}.{}.golden.txt", phase.replace(':', "_"))
}







fn normalize(raw: &str, fixtures: &Path) -> String {
    let mut s = raw.replace("\r\n", "\n");

    let canon = std::fs::canonicalize(fixtures).unwrap_or_else(|_| fixtures.to_path_buf());
    let canon_s = canon.to_string_lossy().to_string();
    let canon_verbatim = canon_s.replace("\\\\?\\", "");
    let canon_fwd = canon_s.replace('\\', "/");
    let canon_verbatim_fwd = canon_verbatim.replace('\\', "/");
    
    let canon_escaped = canon_s.replace('\\', "\\\\");
    let canon_verbatim_escaped = canon_verbatim.replace('\\', "\\\\");

    for p in [
        canon_escaped.as_str(),
        canon_verbatim_escaped.as_str(),
        canon_s.as_str(),
        canon_verbatim.as_str(),
        canon_fwd.as_str(),
        canon_verbatim_fwd.as_str(),
        "crates/varn-pipeline/tests/fixtures/debug",
        "crates\\varn-pipeline\\tests\\fixtures\\debug",
        "../varn-pipeline/tests/fixtures/debug",
        "..\\varn-pipeline\\tests\\fixtures\\debug",
    ] {
        if !p.is_empty() {
            s = s.replace(p, "<fixtures>");
        }
    }
    
    
    s.replace("<fixtures>\\\\", "<fixtures>/")
        .replace("<fixtures>\\", "<fixtures>/")
}

fn run_phase(vn: &str, fixture: &Path, phase: &str) -> String {
    let out = Command::new(vn)
        .arg("debug")
        .arg(fixture)
        .arg("-p")
        .arg(phase)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {vn} debug {fixture:?} -p {phase}: {e}"));

    assert!(
        out.status.success(),
        "`vn debug {fixture:?} -p {phase}` failed (status {:?}):\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );

    let mut raw = String::from_utf8_lossy(&out.stdout).into_owned();
    raw.push_str(&String::from_utf8_lossy(&out.stderr));
    raw
}

fn check_or_update(case: &str, fixture: &Path, phase: &str) {
    let dir = fixtures_dir();
    let vn = env!("CARGO_BIN_EXE_vn");
    let got = normalize(&run_phase(vn, fixture, phase), &dir);
    let golden_path = dir.join(golden_name(case, phase));

    if std::env::var_os("UPDATE_DEBUG_GOLDENS").is_some() {
        std::fs::create_dir_all(&dir).unwrap();
        
        std::fs::write(&golden_path, got.replace("\r\n", "\n")).unwrap();
        return;
    }

    let expected = std::fs::read_to_string(&golden_path).unwrap_or_else(|e| {
        panic!(
            "missing golden {} ({e}); run UPDATE_DEBUG_GOLDENS=1 cargo test -p varn-cli --test debug_golden",
            golden_path.display()
        )
    });

    assert_eq!(
        got.replace("\r\n", "\n"),
        expected.replace("\r\n", "\n"),
        "plain output drifted for fixture '{case}' phase '{phase}'"
    );
}

#[test]
fn debug_plain_goldens() {
    let dir = fixtures_dir();
    for case in FIXTURES {
        let fixture = dir.join(format!("{case}.vn"));
        assert!(fixture.exists(), "missing fixture {}", fixture.display());
        for phase in PHASES {
            check_or_update(case, &fixture, phase);
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn debug_plain_goldens_jit() {
    let dir = fixtures_dir();
    for case in FIXTURES {
        let fixture = dir.join(format!("{case}.vn"));
        for phase in JIT_PHASES {
            check_or_update(case, &fixture, phase);
        }
    }
}
