//! End-to-end evidence for design spec §8, host-API-mismatch variant: a
//! `.vnb` bundle that parses fine (valid envelope + magic) but was built
//! for a different `HOST_API_VERSION` must still hard-error through the
//! real resolution chain — `VARN_STD` -> `provider_impl::active_std` ->
//! `load_bundle_std` -> `StdBundle::validate_compat_with` -> panic — never a
//! silent fallback to the embedded registry.
//!
//! Kept in its own file (own process) rather than a second `#[test]` next to
//! `std_bundle_rejection_bad_magic.rs`: `provider_impl::ACTIVE_STD` is a
//! process-global `OnceLock` that only resolves once per process, so two
//! `#[test]` functions setting different `VARN_STD` values in the same test
//! binary would race / observe a stale cached resolution instead of each
//! getting a fresh one. Cargo already compiles each `tests/*.rs` file as a
//! separate binary, which gives each scenario here a clean process for free.

use varn_modules::artifact::BUILD_FINGERPRINT;
use varn_modules::bundle::{write_bundle, BundleModule, StdBundle};

#[test]
fn host_api_mismatch_bundle_hard_errors_through_real_provider_chain() {
    // Envelope + magic + fingerprint are all valid; only host_api_version
    // is wrong, isolating this test to the `validate_compat_with` failure
    // mode (distinct code path from the bad-magic/`read_bundle` failure
    // covered in the sibling test file).
    let bundle = StdBundle {
        std_version: "0.1.0".into(),
        build_fingerprint: BUILD_FINGERPRINT,
        host_api_version: varn_core::HOST_API_VERSION + 1,
        modules: vec![BundleModule {
            id: "std:math".into(),
            pure: true,
            interface: vec![1, 2, 3],
            bytecode: vec![4, 5, 6],
        }],
    };
    let bytes = write_bundle(&bundle);

    let dir = std::env::temp_dir().join(format!(
        "varn_std_bundle_rejection_host_api_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let bundle_path = dir.join("std.vnb");
    std::fs::write(&bundle_path, &bytes).expect("write mismatched bundle");

    std::env::set_var("VARN_STD", &bundle_path);

    varn_builtins::register_provider();
    let provider = varn_modules::provider::get().expect("provider registered");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        provider.interface_blob("std:math")
    }));

    let err = result.expect_err("host-API-mismatched bundle must panic, not silently fall back");
    let message = err
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
        .expect("panic payload should be a string message");

    assert!(
        message.contains("host API"),
        "panic message did not mention the expected host-API-mismatch reason: {message}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
