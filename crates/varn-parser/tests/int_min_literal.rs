//! `int` is an i64, so its lower bound must be writable. The sign is not part
//! of the literal token — the lexer sees only the magnitude, and
//! `9223372036854775808` does not fit in an i64 — so folding the minus into
//! the literal is the parser's job, and it is the only way i64::MIN can be
//! spelled.

/// Lex and parse one source string. `varn-lexer` is already a dev-dependency
/// of this crate.
fn parses(src: &str) -> bool {
    let (tokens, buf, lex_diags) = varn_lexer::scan(src, "test.vn");
    if !lex_diags.is_empty() {
        return false;
    }
    varn_parser::parse(tokens, buf, "test.vn", varn_core::AtomInterner::new()).is_ok()
}

/// The lower bound of `int` parses.
#[test]
fn i64_min_parses_as_a_literal() {
    assert!(
        parses("let x: int = -9223372036854775808"),
        "i64::MIN must be writable as a literal"
    );
}

/// One past the lower bound is still an error — the fold must not widen the type.
#[test]
fn one_below_i64_min_is_rejected() {
    assert!(
        !parses("let x: int = -9223372036854775809"),
        "a value below i64::MIN must be rejected"
    );
}

/// The magnitude without a sign stays an error: it is above i64::MAX.
#[test]
fn unsigned_magnitude_is_still_rejected() {
    assert!(
        !parses("let x: int = 9223372036854775808"),
        "9223372036854775808 is above i64::MAX and must be rejected"
    );
}

/// Everything that parses today keeps parsing, and a plain negation is
/// untouched by the fold.
#[test]
fn ordinary_negation_still_parses() {
    assert!(parses("let x: int = -5"));
    assert!(parses("let y: int = -(3 + 4)"));
    assert!(parses("let z: int = 9223372036854775807"));
}
