//! El `.vnc` portable: la salida de `vn build`, que el usuario copia y
//! ejecuta en otra máquina con otro binario.
//!
//! Se sella como [`ArtifactClass::Distributable`], así que sólo exige que el
//! esquema case. Las entradas de caché son la otra clase y viven en
//! `cache.rs`; comparten payload pero no reglas de validez.

use crate::PipelineError;
use varn_modules::artifact::{
    read_artifact, write_artifact, write_artifact_file, ArtifactClass, ArtifactKind,
};
use varn_types::ModuleGraphArtifact;

pub fn write_portable(path: &str, artifact: &ModuleGraphArtifact) -> Result<(), PipelineError> {
    let payload = postcard::to_allocvec(artifact)
        .map_err(|e| PipelineError::fatal(format!("cannot serialize .vnc: {e}")))?;

    let out = write_artifact(
        ArtifactKind::ModuleGraph,
        ArtifactClass::Distributable,
        &payload,
    );

    write_artifact_file(std::path::Path::new(path), &out)
        .map_err(|e| PipelineError::fatal(format!("cannot write '{}': {e}", path)))
}

pub fn read_portable(path: &str) -> Result<ModuleGraphArtifact, PipelineError> {
    let bytes = std::fs::read(path)
        .map_err(|e| PipelineError::fatal(format!("cannot read '{}': {e}", path)))?;

    let payload = read_artifact(ArtifactKind::ModuleGraph, &bytes)
        .map_err(|e| PipelineError::fatal(format!("'{}' no se puede ejecutar: {}", path, e)))?;

    postcard::from_bytes(payload)
        .map_err(|e| PipelineError::fatal(format!("cannot deserialize '{}': {e}", path)))
}

pub fn is_portable(path: &str) -> bool {
    path.ends_with(".vnc")
}
