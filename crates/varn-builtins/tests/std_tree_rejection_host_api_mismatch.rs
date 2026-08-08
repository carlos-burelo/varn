//! End-to-end evidence for design spec §3: a std that resolves but cannot be
//! used must produce a hard error through the *real* chain — `VARN_STD` ->
//! `provider_impl::active_std` -> `load_tree_std` -> `std_load_error` — and
//! never a silent fallback to the embedded registry.
//!
//! A source tree declaring the wrong `hostApi` is the reachable form of that
//! failure. The other one — a bundle built by a different compiler — is not
//! reachable any more by construction: the only bundle a `vn` will load is
//! the one compiled into it, so its `build_fingerprint` is its own. That gate
//! is still unit-tested directly in `varn-modules/src/bundle.rs`
//! (`rejects_fingerprint_mismatch`, `rejects_host_api_mismatch`,
//! `rejects_bad_magic`).
//!
//! The failure is reported as data rather than a panic so each host chooses
//! its own loudness (`vn` exits, `vn lsp` reports and keeps serving); what
//! this test pins down is that the reason is available *and* that the
//! rejected std's modules do not resolve anyway.
//!
//! Isolation note: `provider_impl::ACTIVE_STD` is a process-global
//! `OnceLock`, resolved once per process on first use. Cargo compiles each
//! `tests/*.rs` file as its own binary (its own process), which gives this
//! test a fresh, unpolluted `OnceLock` for free. Do not add a second `#[test]`
//! to this file that also touches `VARN_STD`/`active_std`: within one process
//! the `OnceLock` only resolves once, so a second attempt would silently
//! observe the first test's cached result instead of re-resolving.

#[test]
fn host_api_mismatch_tree_hard_errors_through_real_provider_chain() {
    let dir = std::env::temp_dir().join(format!(
        "varn_std_tree_rejection_host_api_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");

    // Valid manifest in every respect except hostApi, isolating this test to
    // the `load_tree_std` version gate.
    let manifest = format!(
        r#"{{"version":"0.1.0","hostApi":{},"modules":[{{"id":"std:math","pure":true}}]}}"#,
        varn_core::HOST_API_VERSION + 1
    );
    std::fs::write(dir.join("std.json"), manifest).expect("write std.json");
    std::fs::write(dir.join("math.vn"), "export const PI = 3;").expect("write math.vn");

    // Override whatever VARN_STD the workspace's .cargo/config.toml set (it
    // points at the source tree in dev) before anything resolves
    // `active_std()`, so this process's active std is unambiguously ours.
    std::env::set_var("VARN_STD", &dir);

    varn_builtins::register_provider();
    let provider = varn_modules::provider::get().expect("provider registered");

    let message = varn_builtins::std_load_error()
        .expect("incompatible std must be reported, not silently ignored");
    assert!(
        message.contains("host API"),
        "error did not mention the expected host-API-mismatch reason: {message}"
    );

    assert!(
        provider.spec_for("std:math").is_none(),
        "rejected std must not fall back to the embedded registry"
    );
    assert!(
        provider.interface_blob("std:math").is_none(),
        "rejected std must not resolve std:math"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
