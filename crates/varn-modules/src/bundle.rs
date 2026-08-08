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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StdBundle {
        StdBundle {
            std_version: "0.1.0".into(),
            build_fingerprint: crate::artifact::BUILD_FINGERPRINT,
            host_api_version: 1,
            modules: vec![BundleModule {
                id: "std:math".into(),
                pure: true,
                interface: vec![1, 2, 3],
                bytecode: vec![4, 5, 6],
                source: "export const PI = 3;".into(),
            }],
        }
    }

    #[test]
    fn roundtrip() {
        let b = sample();
        let bytes = write_bundle(&b);
        let back = read_bundle(&bytes).unwrap();
        assert_eq!(back.std_version, "0.1.0");
        assert_eq!(back.modules.len(), 1);
        assert_eq!(back.modules[0].id, "std:math");
        assert_eq!(back.modules[0].bytecode, vec![4, 5, 6]);
        assert_eq!(back.modules[0].source, "export const PI = 3;");
        assert!(back.validate_compat_with(1).is_ok());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = write_bundle(&sample());
        bytes[0] = b'X';
        assert!(read_bundle(&bytes).is_err());
    }

    #[test]
    fn rejects_fingerprint_mismatch() {
        let mut b = sample();
        b.build_fingerprint = b.build_fingerprint.wrapping_add(1);
        let bytes = write_bundle(&b);
        let back = read_bundle(&bytes).unwrap();
        let err = back.validate_compat_with(1).unwrap_err();
        assert!(err.contains("compiler"), "err was: {err}");
    }

    #[test]
    fn rejects_host_api_mismatch() {
        let mut b = sample();
        b.host_api_version += 1;
        let bytes = write_bundle(&b);
        let err = read_bundle(&bytes).unwrap().validate_compat_with(1).unwrap_err();
        assert!(err.contains("host API"), "err was: {err}");
    }
}
