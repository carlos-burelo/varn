//! `.vnb` std bundle: one versioned artifact carrying every std module's
//! checker interface blob + compiled bytecode blob.
//!
//! Envelope: MAGIC_VNB + VNB_FORMAT_VERSION (artifact.rs). Payload: postcard.
//! Blobs stay opaque `Vec<u8>` here — interface = varn-checker `CachedModule`
//! postcard, bytecode = varn-types `FunctionProto` postcard. Decoding them is
//! the consumer's job (lazy, per module, on first import).

use serde::{Deserialize, Serialize};

use crate::artifact::{
    read_envelope, write_envelope, BUILD_FINGERPRINT, MAGIC_VNB, VNB_FORMAT_VERSION,
};

#[derive(Serialize, Deserialize)]
pub struct StdBundle {
    pub std_version: String,
    /// BUILD_FINGERPRINT of the vn build that produced this bundle. Covers
    /// bytecode/codegen AND checker-interface serialization drift.
    pub build_fingerprint: u32,
    pub host_api_version: u32,
    pub modules: Vec<BundleModule>,
}

#[derive(Serialize, Deserialize)]
pub struct BundleModule {
    pub id: String,
    pub pure: bool,
    pub interface: Vec<u8>,
    pub bytecode: Vec<u8>,
    /// Original `.vn` text. Not used to build anything — `interface` and
    /// `bytecode` already carry the compiled forms — but the editor needs it:
    /// goto-definition, hover and the symbol index all want real source, and
    /// without it a released `vn` can only offer them inside a checkout.
    /// 71 KiB for the whole std; the same trade Rust makes with `rust-src`,
    /// except always shipped rather than an opt-in component.
    pub source: String,
}

pub fn write_bundle(bundle: &StdBundle) -> Vec<u8> {
    let payload = postcard::to_allocvec(bundle).expect("bundle serialization cannot fail");
    write_envelope(MAGIC_VNB, VNB_FORMAT_VERSION, &payload)
}

pub fn read_bundle(bytes: &[u8]) -> Result<StdBundle, String> {
    let payload = read_envelope(MAGIC_VNB, VNB_FORMAT_VERSION, bytes)
        .map_err(|e| format!("invalid std bundle: {e}"))?;
    postcard::from_bytes(payload).map_err(|e| format!("corrupt std bundle: {e}"))
}

impl StdBundle {
    /// Hard compatibility gate — no silent fallback (spec §3).
    /// `host_api_expected` comes from the caller so varn-modules does not
    /// depend on varn-core.
    pub fn validate_compat_with(&self, host_api_expected: u32) -> Result<(), String> {
        if self.build_fingerprint != BUILD_FINGERPRINT {
            return Err(format!(
                "std bundle was built by a different compiler build (fingerprint {:#010x}, this vn is {:#010x}); rebuild vn so the bundle it embeds matches",
                self.build_fingerprint, BUILD_FINGERPRINT
            ));
        }
        if self.host_api_version != host_api_expected {
            return Err(format!(
                "std bundle requires host API v{} but this vn provides v{}",
                self.host_api_version, host_api_expected
            ));
        }
        Ok(())
    }
}