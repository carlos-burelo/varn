//! Golden baseline for what the checker decides.
//!
//! `varn-checker` answers "what is the type of this expression" from more than
//! one engine, and collapsing them into one is only safe if every type it
//! produces today can be compared against every type it produces tomorrow.
//! This test is that comparison.
//!
//! Regenerate after an intentional change:
//!
//! ```text
//! cargo test -p varn-cli --test golden_types -- --ignored update_goldens
//! ```
//!
//! and then READ THE DIFF. A regeneration that nobody looked at is the same as
//! no baseline at all.

use std::path::{Path, PathBuf};

/// Corpus. Small enough to eyeball a diff, wide enough to cover the shapes the
/// checker treats specially: generics, unions, enums/ADTs, tuples and records,
/// narrowing, closures, extensions, and the utility types from `std:types`.
const CORPUS: &[&str] = &[
    "01-arithmetic.vn",
    "05-arrays.vn",
    "07-closures.vn",
    "08-null-safety.vn",
    "14-generics.vn",
    "15-unions.vn",
    "16-enums.vn",
    "23-objects.vn",
    "24-record.vn",
    "26-numeric-coercion.vn",
    "30-nullable-types.vn",
    "36-advanced-generics.vn",
    "53-int48-wrapping.vn",
    "55-array-element-inference.vn",
    "69-tuples-records.vn",
    "70-result-extensions.vn",
    "73-str-charcode-json-shape.vn",
];

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is <repo>/crates/varn-cli.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/varn-cli has two ancestors")
        .to_path_buf()
}

fn golden_dir() -> PathBuf {
    repo_root().join("tests").join("golden").join("types")
}

/// Run the front end far enough to have a `CheckResult`, then render it.
///
/// Deliberately not shelling out to `vn`: a golden test that depends on a
/// built binary cannot run under `cargo test`, which is where CI can gate it.
fn render(test_file: &str) -> String {
    let path = repo_root().join("tests").join(test_file);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    // The checker resolves `std:*` through the builtins provider; without it
    // every stdlib type would come back unresolved and the baseline would
    // record that instead of the real answers.
    varn_builtins::register_provider();

    let filename = path.to_string_lossy().into_owned();
    let (tokens, lexeme_buf, _lex_errs) = varn_lexer::scan(&source, &filename);
    let mut program = varn_parser::parse(tokens, lexeme_buf, &filename)
        .unwrap_or_else(|e| panic!("parse failed for {test_file}: {e:?}"));
    varn_core::assign_ast_ids(&mut program);

    let check = varn_checker::Checker::check(&program);
    varn_debug::expr::render_check_types(&program, &source, &check)
}

#[test]
fn checker_answers_match_the_baseline() {
    let dir = golden_dir();
    let mut stale: Vec<String> = Vec::new();

    for file in CORPUS {
        let actual = render(file);
        let golden_path = dir.join(format!("{file}.txt"));
        let expected = match std::fs::read_to_string(&golden_path) {
            Ok(s) => s,
            Err(_) => {
                stale.push(format!("{file}: no baseline at {}", golden_path.display()));
                continue;
            }
        };
        // Normalise line endings: the goldens are committed with whatever the
        // checkout produced, and a CRLF/LF difference is not a type change.
        if actual.replace("\r\n", "\n") != expected.replace("\r\n", "\n") {
            let first_diff = actual
                .lines()
                .zip(expected.lines())
                .enumerate()
                .find(|(_, (a, b))| a != b)
                .map(|(i, (a, b))| format!("line {}:\n  actual:   {a}\n  expected: {b}", i + 1))
                .unwrap_or_else(|| {
                    format!(
                        "same prefix, different length ({} vs {} lines)",
                        actual.lines().count(),
                        expected.lines().count()
                    )
                });
            stale.push(format!("{file}: {first_diff}"));
        }
    }

    assert!(
        stale.is_empty(),
        "the checker's answers changed for {} file(s):\n\n{}\n\n\
         If the change is intended, regenerate with\n  \
         cargo test -p varn-cli --test golden_types -- --ignored update_goldens\n\
         and read the diff before committing it.",
        stale.len(),
        stale.join("\n\n")
    );
}

#[test]
#[ignore = "writes the baselines; run deliberately"]
fn update_goldens() {
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).expect("cannot create the golden directory");
    for file in CORPUS {
        let rendered = render(file);
        std::fs::write(dir.join(format!("{file}.txt")), rendered).expect("cannot write baseline");
    }
    eprintln!("wrote {} baselines to {}", CORPUS.len(), dir.display());
}
