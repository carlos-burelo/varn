use std::fs;
use varn_modules::resolve_pkg_specifier;

#[test]
fn test_wildcard_and_fallback_pkg_resolution() {
    let temp_dir = std::env::temp_dir().join("varn_pkg_res_test");
    let _ = fs::create_dir_all(&temp_dir);

    // Create a mock package in temp_dir/.vn/packages/my-math
    let pkg_dir = temp_dir.join(".vn").join("packages").join("my-math");
    let src_dir = pkg_dir.join("src");
    let _ = fs::create_dir_all(&src_dir);

    let manifest_content = r#"{
        "name": "my-math",
        "version": "1.2.3",
        "main": "src/index.vn",
        "exports": {
            ".": "./src/index.vn",
            "./helpers/*": "./src/helpers/*.vn"
        }
    }"#;
    fs::write(pkg_dir.join("varn.json"), manifest_content).unwrap();
    fs::write(src_dir.join("index.vn"), "// index").unwrap();

    let helpers_dir = src_dir.join("helpers");
    let _ = fs::create_dir_all(&helpers_dir);
    fs::write(helpers_dir.join("algebra.vn"), "// algebra").unwrap();

    // 1. Resolve root package specifier "pkg:my-math"
    let resolved_root = resolve_pkg_specifier(&temp_dir, "pkg:my-math");
    assert!(resolved_root.is_some(), "should resolve root package via exports '.'");
    assert!(resolved_root.unwrap().ends_with("src/index.vn"));

    // 2. Resolve wildcard specifier "pkg:my-math/helpers/algebra"
    let resolved_wildcard = resolve_pkg_specifier(&temp_dir, "pkg:my-math/helpers/algebra");
    assert!(resolved_wildcard.is_some(), "should resolve wildcard export './helpers/*'");
    assert!(resolved_wildcard.unwrap().ends_with("src/helpers/algebra.vn"));

    // Clean up
    let _ = fs::remove_dir_all(&temp_dir);
}
