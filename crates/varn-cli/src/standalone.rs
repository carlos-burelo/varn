use std::io::{Read, Seek, SeekFrom};

use std::path::Path;

use varn_modules::artifact::{
    read_artifact, write_artifact, ArtifactClass, ArtifactKind, MAGIC_VEXE,
};
use varn_pipeline::{CapabilitySet, Executable, Policy, Session};
use varn_types::ModuleGraphArtifact;

const TRAILER_LEN: u64 = 20;

pub struct Embedded {
    pub std_image: Vec<u8>,
    pub graph: ModuleGraphArtifact,
}

pub fn detect() -> Result<Option<Embedded>, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut file = std::fs::File::open(&exe).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    if len < TRAILER_LEN {
        return Ok(None);
    }

    let mut trailer = [0u8; TRAILER_LEN as usize];
    file.seek(SeekFrom::End(-(TRAILER_LEN as i64)))
        .and_then(|_| file.read_exact(&mut trailer))
        .map_err(|e| e.to_string())?;
    if &trailer[16..20] != MAGIC_VEXE {
        return Ok(None);
    }

    let graph_len = u64::from_le_bytes(trailer[0..8].try_into().map_err(|_| "trailer")?);
    let image_len = u64::from_le_bytes(trailer[8..16].try_into().map_err(|_| "trailer")?);
    let announced = graph_len
        .checked_add(image_len)
        .and_then(|payload| payload.checked_add(TRAILER_LEN))
        .filter(|announced| *announced <= len)
        .ok_or_else(|| "the trailer announces more bytes than the file has".to_owned())?;

    let mut std_image = vec![0u8; image_len as usize];
    let mut envelope = vec![0u8; graph_len as usize];
    file.seek(SeekFrom::Start(len - announced))
        .and_then(|_| file.read_exact(&mut std_image))
        .and_then(|_| file.read_exact(&mut envelope))
        .map_err(|e| e.to_string())?;

    let payload = read_artifact(ArtifactKind::ModuleGraph, &envelope).map_err(|e| e.to_string())?;
    let graph = postcard::from_bytes(payload).map_err(|e| e.to_string())?;
    Ok(Some(Embedded { std_image, graph }))
}

pub fn run(embedded: Embedded) -> i32 {
    crate::bootstrap::init_standalone(embedded.std_image);
    let executable = match Executable::from_graph(embedded.graph) {
        Ok(executable) => executable,
        Err(e) => {
            eprintln!("standalone executable is corrupt: {e}");
            return 1;
        }
    };
    let policy = Policy::new(CapabilitySet::allow_all());
    match varn_pipeline::execute(&Session::detached(), executable, policy) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Standalone runtime error: {e}");
            1
        }
    }
}

pub fn write_executable(
    runtime: &Path,
    out: &Path,
    graph: &ModuleGraphArtifact,
    std_image: &[u8],
) -> Result<(), String> {
    let payload = postcard::to_allocvec(graph).map_err(|e| e.to_string())?;
    let envelope = write_artifact(
        ArtifactKind::ModuleGraph,
        ArtifactClass::Distributable,
        &payload,
    );

    let mut bytes =
        std::fs::read(runtime).map_err(|e| format!("cannot read '{}': {e}", runtime.display()))?;
    bytes.extend_from_slice(std_image);
    bytes.extend_from_slice(&envelope);
    bytes.extend_from_slice(&(envelope.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&(std_image.len() as u64).to_le_bytes());
    bytes.extend_from_slice(MAGIC_VEXE);
    std::fs::write(out, bytes).map_err(|e| format!("cannot write '{}': {e}", out.display()))?;
    make_executable(out)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("cannot mark '{}' executable: {e}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}
