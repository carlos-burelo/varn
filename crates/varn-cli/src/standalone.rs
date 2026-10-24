use std::io::{Read, Seek, SeekFrom};

use varn_modules::artifact::{read_artifact, ArtifactKind, MAGIC_VEXE};
use varn_pipeline::{CapabilitySet, Executable, Policy, Session};
use varn_types::ModuleGraphArtifact;

const TRAILER_LEN: u64 = 12;

pub fn try_run() -> Option<i32> {
    let graph = match read_embedded() {
        Ok(Some(graph)) => graph,
        Ok(None) => return None,
        Err(reason) => {
            eprintln!("standalone executable is corrupt: {reason}");
            return Some(1);
        }
    };
    let executable = match Executable::from_graph(graph) {
        Ok(executable) => executable,
        Err(e) => {
            eprintln!("standalone executable is corrupt: {e}");
            return Some(1);
        }
    };
    let policy = Policy::new(CapabilitySet::allow_all());
    match varn_pipeline::execute(&Session::detached(), executable, policy) {
        Ok(()) => Some(0),
        Err(e) => {
            eprintln!("Standalone runtime error: {e}");
            Some(1)
        }
    }
}

fn read_embedded() -> Result<Option<ModuleGraphArtifact>, String> {
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
    if &trailer[8..12] != MAGIC_VEXE {
        return Ok(None);
    }

    let payload_len = u64::from_le_bytes(trailer[0..8].try_into().map_err(|_| "trailer")?);
    if len < TRAILER_LEN + payload_len {
        return Err("the trailer announces more bytes than the file has".to_owned());
    }
    let mut envelope = vec![0u8; payload_len as usize];
    file.seek(SeekFrom::Start(len - TRAILER_LEN - payload_len))
        .and_then(|_| file.read_exact(&mut envelope))
        .map_err(|e| e.to_string())?;

    let payload = read_artifact(ArtifactKind::ModuleGraph, &envelope).map_err(|e| e.to_string())?;
    postcard::from_bytes(payload)
        .map(Some)
        .map_err(|e| e.to_string())
}
