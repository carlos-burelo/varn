#![allow(unused_crate_dependencies)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

enum Expect {
    Diagnostics {
        tokens: Vec<String>,
        suggestions: Vec<String>,
    },
    Failure {
        fragments: Vec<String>,
    },
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn parse_expect(source: &str) -> Option<Expect> {
    let mut tokens = Vec::new();
    let mut fragments = Vec::new();
    let mut suggestions = Vec::new();
    for line in source
        .lines()
        .map_while(|line| line.trim().strip_prefix("// expect: "))
    {
        if let Some(text) = line.strip_prefix("suggestion: ") {
            suggestions.push(text.to_owned());
            continue;
        }
        let (level, inner) = line.split_once('[')?;
        let inner = inner.strip_suffix(']')?;
        match (level, inner.starts_with("VN")) {
            ("error" | "warning", true) => tokens.push(format!("{level}[{inner}]")),
            ("error", false) => fragments.push(inner.to_owned()),
            _ => return None,
        }
    }
    match (tokens.is_empty(), fragments.is_empty()) {
        (false, true) => Some(Expect::Diagnostics {
            tokens,
            suggestions,
        }),
        (true, false) => Some(Expect::Failure { fragments }),
        (true, true) | (false, false) => None,
    }
}

fn emitted_tokens(text: &str) -> Vec<String> {
    let mut found: Vec<String> = text
        .lines()
        .filter(|line| line.starts_with("error[VN") || line.starts_with("warning[VN"))
        .filter_map(|line| line.find(']').map(|end| line[..=end].to_owned()))
        .collect();
    found.sort();
    found
}

fn invalid_locations(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim_start().strip_prefix("┌─ "))
        .filter(|location| {
            let mut parts = location.rsplit(':');
            let column = parts.next().and_then(|c| c.parse::<u32>().ok());
            let line = parts.next().and_then(|l| l.parse::<u32>().ok());
            column.is_none_or(|c| c == 0) || line.is_none_or(|l| l == 0)
        })
        .map(str::to_owned)
        .collect()
}

fn vn(sub: &str, file: &Path, cache: &Path) -> Output {
    let root = repo_root();
    Command::new(env!("CARGO_BIN_EXE_vn"))
        .current_dir(&root)
        .arg(sub)
        .arg(file.strip_prefix(&root).unwrap_or(file))
        .env(varn_core::env::EnvVar::CacheDir.name(), cache)
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
    let expect = parse_expect(&src).ok_or_else(|| {
        format!(
            "cabecera inválida: {:?}",
            src.lines().next().unwrap_or_default()
        )
    })?;
    match expect {
        Expect::Diagnostics {
            tokens: mut declared,
            suggestions,
        } => {
            declared.sort();
            let out = vn("check", path, cache);
            let text = combined(&out);
            let emitted = emitted_tokens(&text);
            if emitted != declared {
                return Err(format!(
                    "declarados {declared:?}, emitidos {emitted:?}, salida:
{text}"
                ));
            }
            if let Some(missing) = suggestions
                .iter()
                .find(|s| !text.contains(&format!("help: {s}")))
            {
                return Err(format!(
                    "falta la sugerencia {missing:?}, salida:
{text}"
                ));
            }
            let invalid = invalid_locations(&text);
            if !invalid.is_empty() {
                return Err(format!(
                    "posiciones inválidas {invalid:?}, salida:
{text}"
                ));
            }
            if declared.iter().any(|t| t.starts_with("error")) && out.status.success() {
                return Err("hay errores declarados pero `vn check` salió con 0".to_owned());
            }
            Ok(())
        }
        Expect::Failure { fragments } => {
            let checked = vn("check", path, cache);
            let out = if checked.status.success() {
                vn("run", path, cache)
            } else {
                checked
            };
            let text = combined(&out);
            if out.status.success() || fragments.iter().any(|f| !text.contains(f.as_str())) {
                return Err(format!("esperaba fallo {fragments:?}, salida:\n{text}"));
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

#[test]
fn every_diagnostic_code_has_a_fixture() {
    let root = repo_root();
    let mut declared = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(root.join("tests/errors")).expect("tests/errors") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "vn") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("fixture");
        for line in source
            .lines()
            .map_while(|line| line.trim().strip_prefix("// expect: "))
        {
            if let Some(code) = line
                .split_once("[VN")
                .and_then(|(_, rest)| rest.strip_suffix(']'))
                .and_then(|digits| digits.parse::<u32>().ok())
            {
                declared.insert(code);
            }
        }
    }
    let uncovered: Vec<String> = varn_core::ErrorCode::ALL
        .iter()
        .filter(|code| **code != varn_core::ErrorCode::CompilationInternalError)
        .filter(|code| !declared.contains(&code.code_number()))
        .map(ToString::to_string)
        .collect();
    assert!(
        uncovered.is_empty(),
        "codes without a fixture in tests/errors: {uncovered:?}"
    );
}
