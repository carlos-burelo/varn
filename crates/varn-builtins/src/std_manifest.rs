//! std.json manifest of a std source tree.

use std::path::Path;

#[derive(serde::Deserialize)]
pub struct StdManifest {
    pub version: String,
    #[serde(rename = "hostApi")]
    pub host_api: u32,
    pub modules: Vec<StdManifestModule>,
}

#[derive(serde::Deserialize)]
pub struct StdManifestModule {
    pub id: String,
    #[serde(default)]
    pub pure: bool,
}

pub fn read_manifest(root: &Path) -> Option<StdManifest> {
    let raw = std::fs::read_to_string(root.join("std.json")).ok()?;
    match serde_json::from_str(&raw) {
        Ok(m) => Some(m),
        Err(e) => panic!("invalid std.json in {}: {e}", root.display()),
    }
}
