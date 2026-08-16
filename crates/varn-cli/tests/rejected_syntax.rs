//! Constructs the front end must refuse.
//!
//! Each of these parsed happily at some point and then had no honest meaning
//! downstream — the compiler dropped the construct, or the runtime ignored the
//! modifier — so the program ran and gave a wrong answer instead of failing.
//! Accepting syntax that nothing honours is the defect; these tests pin the
//! refusals so they cannot quietly come back.
//!
//! `tests/errors/*.vn` carries an `// expect:` convention that no runner reads,
//! so negative cases live here, where `cargo test` gates them.

/// Everything the front end has to say about `source`, parse errors included.
fn diagnostics(source: &str) -> String {
    varn_builtins::register_provider();

    let filename = "rejected_syntax.vn";
    let (tokens, lexeme_buf, lex_errs) = varn_lexer::scan(source, filename);
    if !lex_errs.is_empty() {
        return format!("{lex_errs:?}");
    }
    let mut program = match varn_parser::parse(tokens, lexeme_buf, filename) {
        Ok(p) => p,
        Err(bag) => return format!("{bag:?}"),
    };
    varn_core::assign_ast_ids(&mut program);

    let check = varn_checker::Checker::check(&program);
    check
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_rejected(source: &str, needle: &str) {
    let out = diagnostics(source);
    assert!(
        out.contains(needle),
        "expected a diagnostic containing {needle:?}, got:\n{out}"
    );
}

fn assert_accepted(source: &str) {
    let out = diagnostics(source);
    assert!(out.is_empty(), "expected no diagnostics, got:\n{out}");
}

const ASYNC_GENERATOR: &str = "`async` generators are not supported";

/// A generator is driven synchronously, so `await` inside one has nothing to
/// suspend into: it used to corrupt the resumed frame
/// (`value is not callable: 0.0000…64 (type: float)`), and without `await` the
/// `async` was simply ignored — the value typed as `Generator<T>` and `next()`
/// returned the item object rather than a task.
#[test]
fn async_generators_are_rejected_in_every_form() {
    assert_rejected("async function* g() { yield 1 }", ASYNC_GENERATOR);
    assert_rejected("class C { async *m() { yield 1 } }", ASYNC_GENERATOR);
    assert_rejected("const o = { async *m() { yield 1 } }", ASYNC_GENERATOR);
    assert_rejected(
        "extension on int { async *m() { yield 1 } }",
        ASYNC_GENERATOR,
    );
    assert_rejected("const f = async function* () { yield 1 }", ASYNC_GENERATOR);
}

#[test]
fn generators_and_async_functions_are_still_fine_apart() {
    assert_accepted("function* g(): Generator<int> { yield 1 }");
    assert_accepted("async function a(): int { return 1 }");
}

/// `HirObjectProp` has no accessor variant, so an accessor written in an object
/// literal is lowered away and the property reads back as `null`.
#[test]
fn accessors_in_object_literals_are_rejected() {
    let needle = "getters and setters are not supported in object literals";
    assert_rejected("const o = { get g(): int { return 1 } }", needle);
    assert_rejected("const o = { set g(v: int) { } }", needle);
}

/// The methods themselves are fine — they lower to `HirObjectProp::Method` —
/// and they must keep type-checking, `return` included.
#[test]
fn methods_in_object_literals_are_accepted() {
    assert_accepted("const o = { m(): int { return 1 } }");
    assert_accepted("const o = { async m(): int { return 1 } }");
}

/// An interface has to be able to describe an async API; `async` on a
/// signature declares an implementation that returns a `Task`.
#[test]
fn interfaces_accept_async_method_signatures() {
    assert_accepted("interface F { async fetch(): int }");
    assert_rejected(
        "interface F { async fetch: int }",
        "unexpected `async` before interface property",
    );
}

/// `async` is also a legal member name, so the modifier is only consumed when
/// something that can start a member follows it.
#[test]
fn async_is_still_usable_as_an_interface_member_name() {
    assert_accepted("interface F { async: int }");
    assert_accepted("interface F { async?: int }");
    assert_accepted("interface F { async(): int }");
}
