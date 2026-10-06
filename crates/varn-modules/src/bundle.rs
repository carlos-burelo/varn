








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
