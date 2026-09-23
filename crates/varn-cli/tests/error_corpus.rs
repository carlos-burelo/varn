//! Corpus negativo: cada `tests/errors/*.vn` declara en su primera línea qué
//! debe rechazarlo. Sin este runner los fixtures eran documentación, no tests.
//!
//! - `// expect: error[VNxxxx]` / `// expect: warning[VNxxxx]`: `vn check`
//!   imprime ese token (y, si es error, sale con código distinto de 0).
//! - `// expect: error[<texto>]` sin `VN`: fallo de una fase sin código
//!   (emisión, runtime); `vn check` o, si compila, `vn run` falla imprimiendo
//!   `<texto>`.

// Un test de integración no usa las dependencias de la librería.
#![allow(unused_crate_dependencies)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

enum Expect {
    Diagnostic { token: String, is_error: bool },
    Failure { text: String },
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn parse_expect(first_line: &str) -> Option<Expect> {
    let rest = first_line.trim().strip_prefix("// expect: ")?;
    let (level, inner) = rest.split_once('[')?;
    let inner = inner.strip_suffix(']')?;
    match (level, inner.starts_with("VN")) {
        ("error" | "warning", true) => Some(Expect::Diagnostic {
            token: format!("{level}[{inner}]"),
            is_error: level == "error",
        }),
        ("error", false) => Some(Expect::Failure {
            text: inner.to_owned(),
        }),
        _ => None,
    }
}

fn vn(sub: &str, file: &Path, cache: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vn"))
        .arg(sub)
        .arg(file)
        .env("VARN_CACHE_DIR", cache)
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn vn")
}

fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn check_fixture(path: &Path, cache: &Path) -> Result<(), String> {
    let src = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let first = src.lines().next().unwrap_or_default();
    let expect = parse_expect(first).ok_or_else(|| format!("cabecera inválida: {first:?}"))?;
    match expect {
        Expect::Diagnostic { token, is_error } => {
            let out = vn("check", path, cache);
            let text = combined(&out);
            if !text.contains(&token) {
                return Err(format!("esperaba {token}, salida:\n{text}"));
            }
            if is_error && out.status.success() {
                return Err(format!("{token} pero `vn check` salió con 0"));
            }
            Ok(())
        }
        Expect::Failure { text: wanted } => {
            let checked = vn("check", path, cache);
            let out = if checked.status.success() {
                vn("run", path, cache)
            } else {
                checked
            };
            let text = combined(&out);
            if out.status.success() || !text.contains(&wanted) {
                return Err(format!("esperaba fallo {wanted:?}, salida:\n{text}"));
            }
            Ok(())
        }
    }
}

#[test]
fn every_error_fixture_is_rejected_as_declared() {
    let root = repo_root();
    let cache = std::env::temp_dir().join(format!("varn-error-corpus-{}", std::process::id()));
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(root.join("tests/errors"))
        .expect("tests/errors")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "vn"))
        .collect();
    fixtures.sort();
    let failures: Vec<String> = fixtures
        .iter()
        .filter_map(|p| {
            check_fixture(p, &cache)
                .err()
                .map(|e| format!("{}: {e}", p.display()))
        })
        .collect();
    let _ = std::fs::remove_dir_all(&cache);
    assert!(
        failures.is_empty(),
        "{} fixture(s):\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
