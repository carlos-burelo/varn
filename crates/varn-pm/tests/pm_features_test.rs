#![allow(unused_crate_dependencies)] // per-target lint: a test needs only a slice of the crate deps

use varn_pm::manifest::DepOrigin;

#[test]
fn test_dep_origin_parsing() {
    let remote = DepOrigin::parse("github.com/carlos-burelo/my-lib@^1.0.0").unwrap();
    assert!(matches!(remote, DepOrigin::Remote { .. }));
    assert_eq!(remote.local_name(), "github_com_carlos-burelo_my-lib");

    let local = DepOrigin::parse("path:../my-local-lib").unwrap();
    assert!(matches!(local, DepOrigin::LocalPath { .. }));
    assert_eq!(local.to_origin_string(), "path:../my-local-lib");
    assert_eq!(local.local_name(), "local_my-local-lib");
}

#[test]
fn test_clean_cache() {
    let result = varn_pm::cache::clean_cache();
    assert!(result.is_ok());
}
