use std::sync::Arc;

use crate::Failure;

#[derive(Clone)]
pub enum Source {
    File { path: Arc<str>, text: Arc<str> },
    Virtual { name: Arc<str>, text: Arc<str> },
}

impl Source {
    pub fn file(path: &str) -> Result<Source, Failure> {
        use varn_modules::loader::ModuleLoader;
        let id = varn_core::ModuleId::local_str(path);
        let text = varn_modules::loader::default_registry()
            .source(&id)
            .map(|s| Arc::<str>::from(s.text.as_ref()))
            .map_err(|e| Failure::io(format!("error[io]: {e}")))?;
        let canonical = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_owned());
        Ok(Source::File {
            path: Arc::from(canonical),
            text,
        })
    }

    pub fn virtual_source(name: &str, text: &str) -> Source {
        Source::Virtual {
            name: Arc::from(name),
            text: Arc::from(text),
        }
    }

    pub fn path(&self) -> &str {
        match self {
            Source::File { path, .. } => path,
            Source::Virtual { name, .. } => name,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Source::File { text, .. } | Source::Virtual { text, .. } => text,
        }
    }
}
