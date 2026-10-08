use rustc_hash::FxHashMap as HashMap;
use std::fmt::{self, Display};
use std::path::PathBuf;
use std::sync::Arc;

use varn_core::ModuleId;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Provenance {
    Memory,

    File(PathBuf),

    Bundle,

    Native,
}

#[derive(Clone)]
pub struct ModuleSource {
    pub id: ModuleId,
    pub text: Arc<str>,
    pub provenance: Provenance,

    pub interface: Option<Arc<[u8]>>,

    pub bytecode: Option<Arc<[u8]>>,
}

impl ModuleSource {
    pub fn from_text(id: ModuleId, text: impl Into<Box<str>>, provenance: Provenance) -> Self {
        Self {
            id,
            text: Arc::from(text.into()),
            provenance,
            interface: None,
            bytecode: None,
        }
    }

    pub fn with_interface(mut self, interface: Arc<[u8]>) -> Self {
        self.interface = Some(interface);
        self
    }

    pub fn with_bytecode(mut self, bytecode: Arc<[u8]>) -> Self {
        self.bytecode = Some(bytecode);
        self
    }
}

impl fmt::Debug for ModuleSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModuleSource")
            .field("id", &self.id)
            .field("provenance", &self.provenance)
            .field("bytes", &self.text.len())
            .field("interface", &self.interface.as_ref().map(|b| b.len()))
            .field("bytecode", &self.bytecode.as_ref().map(|b| b.len()))
            .finish()
    }
}

#[derive(Debug)]
pub enum LoadError {
    NotFound { id: ModuleId },

    Io { path: PathBuf, message: String },

    Resolve { specifier: String, message: String },

    Invalid { id: ModuleId, message: String },
}

impl Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NotFound { id } => write!(f, "module not found: {id:?}"),
            LoadError::Io { path, message } => {
                write!(f, "cannot read '{}': {message}", path.display())
            }
            LoadError::Resolve { specifier, message } => {
                write!(f, "cannot resolve '{specifier}': {message}")
            }
            LoadError::Invalid { id, message } => {
                write!(f, "invalid module {id:?}: {message}")
            }
        }
    }
}

impl std::error::Error for LoadError {}

pub trait ModuleLoader: Send + Sync {
    fn resolve(&self, specifier: &str, from: &ModuleId) -> Result<ModuleId, LoadError> {
        crate::resolver::ModuleResolver::new()
            .resolve(specifier, from)
            .map_err(|message| LoadError::Resolve {
                specifier: specifier.to_owned(),
                message,
            })
    }

    fn source(&self, id: &ModuleId) -> Result<ModuleSource, LoadError>;
}

pub struct ProviderLoader;

impl ProviderLoader {
    fn specifier(id: &ModuleId) -> Option<&str> {
        match id {
            ModuleId::Core(s) | ModuleId::Std(s) | ModuleId::Runtime(s) => Some(s.as_ref()),
            ModuleId::Local(_) | ModuleId::Package { .. } => None,
        }
    }
}

impl ModuleLoader for ProviderLoader {
    fn source(&self, id: &ModuleId) -> Result<ModuleSource, LoadError> {
        let Some(spec) = Self::specifier(id) else {
            return Err(LoadError::NotFound { id: id.clone() });
        };
        let Some(provider) = crate::provider::get() else {
            return Err(LoadError::NotFound { id: id.clone() });
        };

        let interface = provider
            .interface_blob(spec)
            .map(|b| Arc::from(b) as Arc<[u8]>);
        let bytecode = provider
            .bytecode_blob(spec)
            .map(|b| Arc::from(b) as Arc<[u8]>);

        if let Some(text) = provider
            .embedded_source(spec)
            .or_else(|| provider.bundled_source(spec))
        {
            let mut src = ModuleSource::from_text(id.clone(), text, Provenance::Bundle);
            src.interface = interface;
            src.bytecode = bytecode;
            return Ok(src);
        }
        if let Some(path) = provider.source_path(spec) {
            let text = std::fs::read_to_string(&path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    LoadError::NotFound { id: id.clone() }
                } else {
                    LoadError::Io {
                        path: path.clone(),
                        message: e.to_string(),
                    }
                }
            })?;
            let mut src = ModuleSource::from_text(id.clone(), text, Provenance::File(path));
            src.interface = interface;
            src.bytecode = bytecode;
            return Ok(src);
        }
        Err(LoadError::NotFound { id: id.clone() })
    }
}

pub fn default_registry() -> ModuleRegistry {
    ModuleRegistry::new()
        .with(Box::new(FilesystemLoader))
        .with(Box::new(ProviderLoader))
}

pub struct ModuleRegistry {
    backends: Vec<Box<dyn ModuleLoader>>,
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self {
            backends: Vec::new(),
        }
    }

    pub fn push(&mut self, backend: Box<dyn ModuleLoader>) -> &mut Self {
        self.backends.push(backend);
        self
    }

    pub fn with(mut self, backend: Box<dyn ModuleLoader>) -> Self {
        self.backends.push(backend);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }
}

impl ModuleLoader for ModuleRegistry {
    fn resolve(&self, specifier: &str, from: &ModuleId) -> Result<ModuleId, LoadError> {
        let mut last: Option<LoadError> = None;
        for backend in &self.backends {
            match backend.resolve(specifier, from) {
                Ok(id) => return Ok(id),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or(LoadError::Resolve {
            specifier: specifier.to_owned(),
            message: "no loader registered".to_owned(),
        }))
    }

    fn source(&self, id: &ModuleId) -> Result<ModuleSource, LoadError> {
        let mut last: Option<LoadError> = None;
        for backend in &self.backends {
            match backend.source(id) {
                Ok(src) => return Ok(src),

                Err(LoadError::NotFound { .. }) => {
                    last = Some(LoadError::NotFound { id: id.clone() })
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or(LoadError::NotFound { id: id.clone() }))
    }
}

pub struct FilesystemLoader;

impl ModuleLoader for FilesystemLoader {
    fn source(&self, id: &ModuleId) -> Result<ModuleSource, LoadError> {
        let ModuleId::Local(path) = id else {
            return Err(LoadError::NotFound { id: id.clone() });
        };
        let path = PathBuf::from(path.as_ref());
        let text = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                LoadError::NotFound { id: id.clone() }
            } else {
                LoadError::Io {
                    path: path.clone(),
                    message: e.to_string(),
                }
            }
        })?;
        Ok(ModuleSource::from_text(
            id.clone(),
            text,
            Provenance::File(path),
        ))
    }
}

#[derive(Default)]
pub struct MemoryLoader {
    files: HashMap<ModuleId, Arc<str>>,
}

impl MemoryLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: ModuleId, text: impl Into<Box<str>>) {
        self.files.insert(id, Arc::from(text.into()));
    }

    pub fn remove(&mut self, id: &ModuleId) -> Option<Arc<str>> {
        self.files.remove(id)
    }
}

impl ModuleLoader for MemoryLoader {
    fn source(&self, id: &ModuleId) -> Result<ModuleSource, LoadError> {
        match self.files.get(id) {
            Some(text) => Ok(ModuleSource::from_text(
                id.clone(),
                text.to_string(),
                Provenance::Memory,
            )),
            None => Err(LoadError::NotFound { id: id.clone() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(path: &str) -> ModuleId {
        ModuleId::local_str(path)
    }

    #[test]
    fn resolve_is_canonical_for_every_scheme() {
        let loader = FilesystemLoader;
        let from = local("C:/proj/main.vn");

        let rel = loader.resolve("./util.vn", &from).unwrap();
        assert_eq!(rel, local("C:/proj/util.vn"));

        let up = loader.resolve("../lib/x.vn", &from).unwrap();
        assert_eq!(up, local("C:/lib/x.vn"));

        assert_eq!(
            loader.resolve("std:math", &from).unwrap(),
            ModuleId::stdlib("std:math")
        );

        assert_eq!(
            loader.resolve("core:types/int", &from).unwrap(),
            ModuleId::core("core:types/int")
        );
        assert_eq!(
            loader.resolve("runtime:fs", &from).unwrap(),
            ModuleId::Runtime("runtime:fs".into())
        );
    }

    #[test]
    fn filesystem_reads_local_and_not_std() {
        let dir = std::env::temp_dir().join(format!(
            "varn-loader-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.vn");
        std::fs::write(&file, "export let x: int = 1\n").unwrap();

        let loader = FilesystemLoader;
        let id = ModuleId::local(&file);
        let src = loader.source(&id).unwrap();
        assert_eq!(src.id, id);
        assert!(src.text.contains("export let x"));
        assert!(matches!(src.provenance, Provenance::File(_)));
        assert!(src.interface.is_none() && src.bytecode.is_none());

        assert!(matches!(
            loader.source(&ModuleId::stdlib("std:math")),
            Err(LoadError::NotFound { .. })
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn memory_overlay_wins_over_filesystem() {
        let dir = std::env::temp_dir().join(format!(
            "varn-loader-overlay-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("doc.vn");
        std::fs::write(&file, "export let y: int = 1\n").unwrap();
        let id = ModuleId::local(&file);

        let mut memory = MemoryLoader::new();
        memory.insert(id.clone(), "export let y: int = 2\n");

        let registry = ModuleRegistry::new()
            .with(Box::new(memory))
            .with(Box::new(FilesystemLoader));

        let src = registry.source(&id).unwrap();
        assert!(src.text.contains("= 2"), "el buffer debe ganar al archivo");
        assert_eq!(src.provenance, Provenance::Memory);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_skips_not_found_and_aggregates_failure() {
        let registry = ModuleRegistry::new()
            .with(Box::new(MemoryLoader::new()))
            .with(Box::new(FilesystemLoader));
        assert!(matches!(
            registry.source(&local("C:/nope/missing.vn")),
            Err(LoadError::NotFound { .. })
        ));
        assert!(registry.resolve("std:math", &local("C:/main.vn")).is_ok());
    }
}
