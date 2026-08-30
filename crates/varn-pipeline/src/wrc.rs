use crate::PipelineError;
use varn_types::ModuleGraphArtifact;

pub fn write_wrc(path: &str, artifact: &ModuleGraphArtifact) -> Result<(), PipelineError> {
    let payload = postcard::to_allocvec(artifact)
        .map_err(|e| PipelineError::fatal(format!("cannot serialize .vnc: {e}")))?;

    let out = varn_modules::artifact::write_artifact(
        varn_modules::artifact::ArtifactKind::ModuleGraph,
        varn_modules::artifact::ArtifactClass::Distributable,
        &payload,
    );

    std::fs::write(path, &out)
        .map_err(|e| PipelineError::fatal(format!("cannot write '{}': {e}", path)))
}

pub fn read_wrc(path: &str) -> Result<ModuleGraphArtifact, PipelineError> {
    let bytes = std::fs::read(path)
        .map_err(|e| PipelineError::fatal(format!("cannot read '{}': {e}", path)))?;

    let payload = varn_modules::artifact::read_artifact(
        varn_modules::artifact::ArtifactKind::ModuleGraph,
        &bytes,
    )
    .map_err(|e| PipelineError::fatal(format!("'{}' no se puede ejecutar: {}", path, e)))?;

    let artifact: ModuleGraphArtifact = postcard::from_bytes(payload)
        .map_err(|e| PipelineError::fatal(format!("cannot deserialize '{}': {e}", path)))?;

    Ok(artifact)
}

pub fn is_wrc(path: &str) -> bool {
    path.ends_with(".vnc")
}
