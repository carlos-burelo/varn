







fn parses(src: &str) -> bool {
    let (tokens, buf, lex_diags) = varn_lexer::scan(src, "test.vn");
    if !lex_diags.is_empty() {
        return false;
    }
    varn_parser::parse(tokens, buf, "test.vn", varn_core::AtomInterner::new()).is_ok()
}


#[test]
fn i64_min_parses_as_a_literal() {
    assert!(
        parses("let x: int = -9223372036854775808"),
        "i64::MIN must be writable as a literal"
    );
}


#[test]
fn one_below_i64_min_is_rejected() {
    assert!(
        !parses("let x: int = -9223372036854775809"),
        "a value below i64::MIN must be rejected"
    );
}


#[test]
fn unsigned_magnitude_is_still_rejected() {
    assert!(
        !parses("let x: int = 9223372036854775808"),
        "9223372036854775808 is above i64::MAX and must be rejected"
    );
}



#[test]
fn ordinary_negation_still_parses() {
    assert!(parses("let x: int = -5"));
    assert!(parses("let y: int = -(3 + 4)"));
    assert!(parses("let z: int = 9223372036854775807"));
}
