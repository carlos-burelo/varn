use crate::error::CliError;
use varn_types::ModuleGraphArtifact;

const MAGIC: &[u8; 4] = b"WRC\0";

pub fn write_wrc(path: &str, artifact: &ModuleGraphArtifact) -> Result<(), CliError> {
    let payload = postcard::to_allocvec(artifact)
        .map_err(|e| CliError::fatal(format!("cannot serialize .wrc: {e}")))?;

    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&artifact.format_version.to_le_bytes());
    out.extend_from_slice(&payload);

    std::fs::write(path, &out).map_err(|e| CliError::fatal(format!("cannot write '{}': {e}", path)))
}

pub fn read_wrc(path: &str) -> Result<ModuleGraphArtifact, CliError> {
    let bytes =
        std::fs::read(path).map_err(|e| CliError::fatal(format!("cannot read '{}': {e}", path)))?;

    if bytes.len() < 8 {
        return Err(CliError::fatal(format!(
            "'{}' is not a valid .wrc file",
            path
        )));
    }
    if &bytes[..4] != MAGIC {
        return Err(CliError::fatal(format!(
            "'{}' is not a valid .wrc file (bad magic)",
            path
        )));
    }

    let file_version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if file_version != super::compile::CACHE_FORMAT_VERSION {
        return Err(CliError::fatal(format!(
            "'{}' was compiled with format version {file_version}, runtime expects {}. Recompile with `vn build`.",
            path,
            super::compile::CACHE_FORMAT_VERSION
        )));
    }

    let artifact: ModuleGraphArtifact = postcard::from_bytes(&bytes[8..])
        .map_err(|e| CliError::fatal(format!("cannot deserialize '{}': {e}", path)))?;

    Ok(artifact)
}

pub fn is_wrc(path: &str) -> bool {
    path.ends_with(".wrc")
}
