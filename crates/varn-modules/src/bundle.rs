//! `.vnb` std bundle: one versioned artifact carrying every std module's
//! checker interface blob + compiled bytecode blob.
//!
//! Envelope: la cabecera única de `artifact.rs`, kind `StdBundle`, clase
//! `Distributable` — el bundle se embebe en el binario y viaja con él.
//! Blobs stay opaque `Vec<u8>` here — interface = varn-checker `CachedModule`
//! postcard, bytecode = varn-types `FunctionProto` postcard. Decoding them is
//! the consumer's job (lazy, per module, on first import).

use serde::{Deserialize, Serialize};

use crate::artifact::{read_artifact, write_artifact, ArtifactClass, ArtifactKind};

#[derive(Serialize, Deserialize)]
pub struct StdBundle {
    pub std_version: String,
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
    write_artifact(
        ArtifactKind::StdBundle,
        ArtifactClass::Distributable,
        &payload,
    )
}

pub fn read_bundle(bytes: &[u8]) -> Result<StdBundle, String> {
    let payload = read_artifact(ArtifactKind::StdBundle, bytes)
        .map_err(|e| format!("bundle de stdlib inválido: {e}"))?;
    postcard::from_bytes(payload).map_err(|e| format!("corrupt std bundle: {e}"))
}

impl StdBundle {
    /// Verja de compatibilidad dura — sin caída silenciosa (spec §3).
    /// `host_api_expected` lo pasa el llamante para que varn-modules no
    /// dependa de varn-core.
    ///
    /// La deriva de esquema ya NO se comprueba aquí: la envolvente sella
    /// `BUILD_FINGERPRINT` y `read_bundle` rechaza el bundle antes de llegar a
    /// deserializarlo. Repetir el dato dentro del payload significaba validar
    /// dos veces lo mismo por dos caminos que podían discrepar.
    pub fn validate_compat_with(&self, host_api_expected: u32) -> Result<(), String> {
        if self.host_api_version != host_api_expected {
            return Err(format!(
                "std bundle requires host API v{} but this vn provides v{}",
                self.host_api_version, host_api_expected
            ));
        }
        Ok(())
    }
}