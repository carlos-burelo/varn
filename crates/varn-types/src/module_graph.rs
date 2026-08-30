use std::collections::HashMap;

use crate::chunk::FunctionProto;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PackageNode {
    pub specifier: String,
    pub package: String,
    pub version: String,
    pub subpath: String,
    pub resolved_path: String,
    pub integrity: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ModuleGraphArtifact {
    pub entry_path: String,
    pub graph_hash: u64,
    pub source_hashes: HashMap<String, u64>,
    pub modules: HashMap<String, FunctionProto>,
    #[serde(default)]
    pub package_nodes: Vec<PackageNode>,
}

impl ModuleGraphArtifact {
    pub fn entry_proto(&self) -> Option<&FunctionProto> {
        self.modules.get(&self.entry_path)
    }
}
