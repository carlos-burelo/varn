//! End-to-end evidence for design spec §8: a corrupt `.vnb` bundle must
//! produce a hard error through the *real* resolution chain — `VARN_STD` ->
//! `provider_impl::active_std` -> `load_bundle_std` -> `read_bundle` ->
//! `std_load_error` — never a silent fallback to the embedded registry.
//!
//! The failure is reported as data rather than a panic so that each host
//! chooses its own loudness (`vn` exits, `vn-lsp` reports and keeps serving);
//! what the test pins down is that the reason is available *and* that the
//! rejected bundle's modules do not resolve anyway.
//!
//! `crates/varn-modules/src/bundle.rs` already unit-tests `read_bundle`/
//! `validate_compat_with` directly (`rejects_bad_magic`,
//! `rejects_fingerprint_mismatch`, `rejects_host_api_mismatch`). This test
//! exercises the whole chain instead: a real bundle file, on disk, resolved
//! through `VARN_STD` and the registered `StdlibProvider`, the same way the
//! checker (`varn-checker/src/module_resolver.rs::resolve_from_interface_blob`)
//! resolves it in production.
//!
//! Isolation note: `provider_impl::ACTIVE_STD` is a process-global
//! `OnceLock`, resolved once per process on first use. Cargo compiles each
//! `tests/*.rs` file as its own binary (its own process), which gives this
//! test — and the sibling host-API-mismatch test in a separate file — a
//! fresh, unpolluted `OnceLock` for free. Do not add a second `#[test]` to
//! this file that also touches `VARN_STD`/`active_std`: within one process
//! the `OnceLock` only resolves once, so a second attempt would silently
//! observe the first test's cached result instead of re-resolving.

use varn_modules::artifact::BUILD_FINGERPRINT;
use varn_modules::bundle::{write_bundle, BundleModule, StdBundle};

fn sample_bundle() -> StdBundle {
    StdBundle {
        std_version: "0.1.0".into(),
        build_fingerprint: BUILD_FINGERPRINT,
        host_api_version: varn_core::HOST_API_VERSION,
        modules: vec![BundleModule {
            id: "std:math".into(),
            pure: true,
            interface: vec![1, 2, 3],
            bytecode: vec![4, 5, 6],
        }],
    }
}

#[test]
fn corrupt_magic_bundle_hard_errors_through_real_provider_chain() {
    // Build a bundle that is otherwise perfectly valid (matching
    // fingerprint + host API), then flip the magic byte exactly like
    // `bundle.rs::rejects_bad_magic` does — isolating this test to *only*
    // the envelope-magic failure mode.
    let mut bytes = write_bundle(&sample_bundle());
    bytes[0] = b'X';

    let dir = std::env::temp_dir().join(format!(
        "varn_std_bundle_rejection_bad_magic_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let bundle_path = dir.join("std.vnb");
    std::fs::write(&bundle_path, &bytes).expect("write corrupt bundle");

    // Override whatever VARN_STD the workspace's .cargo/config.toml set
    // (it points at the source tree in dev) before anything resolves
    // `active_std()`, so this process's active std is unambiguously our
    // corrupt bundle file.
    std::env::set_var("VARN_STD", &bundle_path);

    varn_builtins::register_provider();
    let provider = varn_modules::provider::get().expect("provider registered");

    let message = varn_builtins::std_load_error()
        .expect("corrupt bundle must be reported, not silently ignored");
    assert!(
        message.contains("invalid std bundle"),
        "error did not mention the expected bundle-rejection reason: {message}"
    );

    // `interface_blob` is the real trigger the checker uses in production
    // (see `varn-checker/src/module_resolver.rs::resolve_from_interface_blob`)
    // — a rejected bundle must leave it empty rather than let the embedded
    // registry quietly stand in for the module.
    assert!(
        provider.interface_blob("std:math").is_none(),
        "rejected bundle must not resolve std:math"
    );
    assert!(
        provider.spec_for("std:math").is_none(),
        "rejected bundle must not fall back to the embedded registry"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
