use std::path::{Path, PathBuf};

use varn_core::ModuleId;

pub const ENV_DIR_NAME: &str = ".vn";
pub const CACHE_DIR_NAME: &str = "cache";
pub const BYTECODE_DIR_NAME: &str = "bytecode";
pub const TYPES_DIR_NAME: &str = "types";
pub const PACKAGE_MANIFEST_FILE: &str = "varn.toml";
pub const PACKAGE_MANIFEST_FILE_VN: &str = "vn.toml";

include!(concat!(env!("OUT_DIR"), "/build_fingerprint.rs"));

pub fn producer_fingerprint() -> u32 {
    static FP: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *FP.get_or_init(|| {
        let mut hash: u64 = 0xcbf29ce484222325;
        let mut eat = |bytes: &[u8]| {
            for b in bytes {
                hash ^= *b as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
        };
        match std::env::current_exe() {
            Ok(exe) => {
                eat(exe.to_string_lossy().as_bytes());
                if let Ok(meta) = std::fs::metadata(&exe) {
                    eat(&meta.len().to_le_bytes());
                    if let Ok(modified) = meta.modified() {
                        if let Ok(since) = modified.duration_since(std::time::UNIX_EPOCH) {
                            eat(&since.as_nanos().to_le_bytes());
                        }
                    }
                }
            }

            Err(_) => eat(b"varn-unknown-producer"),
        }
        (hash ^ (hash >> 32)) as u32
    })
}

pub fn cache_key() -> u32 {
    BUILD_FINGERPRINT ^ producer_fingerprint()
}

pub fn prune_superseded(current: &Path) {
    let (Some(dir), Some(name), Some(ext)) = (
        current.parent(),
        current.file_name().and_then(|n| n.to_str()),
        current.extension().and_then(|e| e.to_str()),
    ) else {
        return;
    };

    let suffix = format!(".{ext}");
    let Some(without_ext) = name.strip_suffix(&suffix) else {
        return;
    };
    let Some(dot) = without_ext.rfind('.') else {
        return;
    };
    let prefix = without_ext[..=dot].to_owned();

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut siblings: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let other = path.file_name()?.to_str()?;
            if !other.starts_with(&prefix) || !other.ends_with(&suffix) {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    if siblings.len() <= ARTIFACT_GENERATIONS {
        return;
    }
    siblings.sort_by_key(|b| std::cmp::Reverse(b.0));
    for (_, path) in siblings.drain(ARTIFACT_GENERATIONS..) {
        let _ = std::fs::remove_file(path);
    }
}

pub const ARTIFACT_GENERATIONS: usize = 3;

pub const MAGIC: &[u8; 4] = b"VARN";

pub const MAGIC_VEXE: &[u8; 4] = b"VEXE";

const ENVELOPE_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ArtifactKind {
    ModuleGraph = 1,

    CheckerInterface = 2,

    StdBundle = 3,
}

impl ArtifactKind {
    fn from_u16(v: u16) -> Option<Self> {
        match v {
            1 => Some(Self::ModuleGraph),
            2 => Some(Self::CheckerInterface),
            3 => Some(Self::StdBundle),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::ModuleGraph => "grafo de modulos",
            Self::CheckerInterface => "interfaz de checker",
            Self::StdBundle => "bundle de stdlib",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ArtifactClass {
    Cache = 1,

    Distributable = 2,
}

impl ArtifactClass {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Cache),
            2 => Some(Self::Distributable),
            _ => None,
        }
    }

    fn producer_stamp(self) -> u32 {
        match self {
            Self::Cache => producer_fingerprint(),
            Self::Distributable => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactError {
    TooShort,
    BadMagic,

    UnknownEnvelope(u16),
    UnknownKind(u16),
    UnknownClass(u8),

    WrongKind {
        expected: ArtifactKind,
        found: ArtifactKind,
    },

    Superseded,

    Corrupt,
}

impl std::fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort => write!(f, "artefacto truncado"),
            Self::BadMagic => write!(f, "no es un artefacto de Varn"),
            Self::UnknownEnvelope(v) => {
                write!(f, "cabecera de artefacto v{v}, no reconocida por este vn")
            }
            Self::UnknownKind(k) => write!(f, "tipo de artefacto desconocido ({k})"),
            Self::UnknownClass(c) => write!(f, "clase de artefacto desconocida ({c})"),
            Self::WrongKind { expected, found } => write!(
                f,
                "se esperaba {}, el archivo lleva {}",
                expected.name(),
                found.name()
            ),
            Self::Superseded => write!(f, "compilado por otra version de Varn"),
            Self::Corrupt => write!(f, "artefacto corrupto (checksum no coincide)"),
        }
    }
}

fn payload_checksum(payload: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for &b in payload {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

pub fn find_project_root(start_path: &Path) -> PathBuf {
    start_path
        .ancestors()
        .find(|dir| {
            dir.join(PACKAGE_MANIFEST_FILE).exists() || dir.join(PACKAGE_MANIFEST_FILE_VN).exists()
        })
        .unwrap_or(start_path)
        .to_path_buf()
}

pub fn global_varn_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VARN_HOME") {
        return PathBuf::from(dir);
    }
    if let Some(home) = dirs_home() {
        return home.join(".varn");
    }
    PathBuf::from(".varn")
}

pub fn get_cache_dir(_project_root: &Path) -> PathBuf {
    if let Ok(dir) = std::env::var("VARN_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    global_varn_dir().join(CACHE_DIR_NAME)
}

pub fn get_bytecode_cache_dir(project_root: &Path) -> PathBuf {
    get_cache_dir(project_root).join(BYTECODE_DIR_NAME)
}

pub fn get_types_cache_dir(project_root: &Path) -> PathBuf {
    get_cache_dir(project_root).join(TYPES_DIR_NAME)
}

pub fn source_fingerprint(source: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

pub fn module_key(id: &ModuleId, fingerprint: u64) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut h);
    format!("{:016x}_{:016x}", h.finish(), fingerprint)
}

pub fn module_artifact_path(
    dir: &Path,
    kind: ArtifactKind,
    id: &ModuleId,
    fingerprint: u64,
) -> PathBuf {
    dir.join(format!(
        "{}_{}.vnm",
        kind as u16,
        module_key(id, fingerprint)
    ))
}

pub fn write_module_artifact(
    dir: &Path,
    kind: ArtifactKind,
    id: &ModuleId,
    fingerprint: u64,
    payload: &[u8],
) {
    let path = module_artifact_path(dir, kind, id, fingerprint);
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let bytes = write_artifact(kind, ArtifactClass::Cache, payload);
    if write_artifact_file(&path, &bytes).is_ok() {
        prune_superseded(&path);
    }
}

pub fn read_module_artifact(
    dir: &Path,
    kind: ArtifactKind,
    id: &ModuleId,
    fingerprint: u64,
) -> Option<Vec<u8>> {
    let path = module_artifact_path(dir, kind, id, fingerprint);
    let bytes = std::fs::read(path).ok()?;
    read_artifact(kind, &bytes).ok().map(|p| p.to_vec())
}

fn dirs_home() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

const HEADER_LEN: usize = 21;

pub fn write_artifact(kind: ArtifactKind, class: ArtifactClass, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&ENVELOPE_VERSION.to_le_bytes());
    out.extend_from_slice(&(kind as u16).to_le_bytes());
    out.push(class as u8);
    out.extend_from_slice(&BUILD_FINGERPRINT.to_le_bytes());
    out.extend_from_slice(&class.producer_stamp().to_le_bytes());
    out.extend_from_slice(&payload_checksum(payload).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn read_artifact(expected: ArtifactKind, bytes: &[u8]) -> Result<&[u8], ArtifactError> {
    if bytes.len() < HEADER_LEN {
        return Err(ArtifactError::TooShort);
    }
    if &bytes[..4] != MAGIC {
        return Err(ArtifactError::BadMagic);
    }
    let Ok(envelope) = bytes[4..6].try_into().map(u16::from_le_bytes) else {
        return Err(ArtifactError::Corrupt);
    };
    if envelope != ENVELOPE_VERSION {
        return Err(ArtifactError::UnknownEnvelope(envelope));
    }
    let Ok(raw_kind) = bytes[6..8].try_into().map(u16::from_le_bytes) else {
        return Err(ArtifactError::Corrupt);
    };
    let Some(kind) = ArtifactKind::from_u16(raw_kind) else {
        return Err(ArtifactError::UnknownKind(raw_kind));
    };
    if kind != expected {
        return Err(ArtifactError::WrongKind {
            expected,
            found: kind,
        });
    }
    let Some(class) = ArtifactClass::from_u8(bytes[8]) else {
        return Err(ArtifactError::UnknownClass(bytes[8]));
    };
    let Ok(schema) = bytes[9..13].try_into().map(u32::from_le_bytes) else {
        return Err(ArtifactError::Corrupt);
    };
    if schema != BUILD_FINGERPRINT {
        return Err(ArtifactError::Superseded);
    }
    let Ok(producer) = bytes[13..17].try_into().map(u32::from_le_bytes) else {
        return Err(ArtifactError::Corrupt);
    };
    if producer != class.producer_stamp() {
        return Err(ArtifactError::Superseded);
    }
    let Ok(expected_sum) = bytes[17..21].try_into().map(u32::from_le_bytes) else {
        return Err(ArtifactError::Corrupt);
    };
    let payload = &bytes[HEADER_LEN..];

    if payload_checksum(payload) != expected_sum {
        return Err(ArtifactError::Corrupt);
    }
    Ok(payload)
}

pub fn write_artifact_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp{}_{}", std::process::id(), seq));
    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}
