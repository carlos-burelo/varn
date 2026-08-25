#![allow(unused_crate_dependencies)]

//! `get_members_of_type` documents itself as returning "every member reachable
//! on `ty`". An `extension` block declares members that are exactly as reachable
//! as any other, and they used to be missing — so the language server carried a
//! parallel table of them that only tooling could see.

use varn_checker::module_resolver::DiskResolver;
use varn_checker::{get_members_of_type, Checker, CheckOptions, ResolvedMemberKind, Type};

// Syntax copied from `tests/25-extensions.vn` rather than guessed: methods have
// no `fn`, and the receiver is named after `on`.
const SRC: &str = "extension StringUtils on str {\n\
                   \x20   shout(): str { return this + \"!\" }\n\
                   \x20   get loud(): str { return this }\n\
                   }\n\
                   const s: str = \"hola\"\n";

fn members_of(ty: &Type) -> Vec<(String, ResolvedMemberKind)> {
    let (tokens, buf, _) = varn_lexer::scan(SRC, "<ext>");
    let program = varn_parser::parse(tokens, buf, "<ext>").expect("fixture must parse");
    let resolver = DiskResolver::new();
    let check = Checker::check_with(&program, &resolver, CheckOptions::tooling());
    get_members_of_type(&resolver, ty, &check.bind)
        .into_iter()
        .map(|m| (m.name.to_string(), m.kind))
        .collect()
}

#[test]
fn extension_methods_are_reachable_members() {
    let found = members_of(&Type::Str);
    let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();

    assert!(
        names.contains(&"shout"),
        "an extension method must be a member of the type it extends; got {names:?}"
    );
    assert!(
        names.contains(&"loud"),
        "an extension getter must be a member of the type it extends; got {names:?}"
    );
}

/// They are labelled as extensions, so a caller can tell them apart from
/// members the type declares itself.
#[test]
fn extension_members_carry_their_kind() {
    let found = members_of(&Type::Str);
    let shout = found
        .iter()
        .find(|(n, _)| n == "shout")
        .expect("`shout` must be present");
    assert_eq!(shout.1, ResolvedMemberKind::ExtensionMethod);
}

/// The intrinsic `str` and the named `str` must agree: the intrinsic arm
/// delegates to the named one, and a `return` there once skipped the extension
/// pass entirely.
#[test]
fn the_intrinsic_and_named_forms_agree() {
    let via_intrinsic = members_of(&Type::Str);
    let via_named = members_of(&Type::named("str".to_owned()));

    let a: Vec<&str> = via_intrinsic.iter().map(|(n, _)| n.as_str()).collect();
    let b: Vec<&str> = via_named.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(a, b, "both spellings of `str` must expose the same members");
}
