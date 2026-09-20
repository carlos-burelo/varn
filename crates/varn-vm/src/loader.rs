use std::fmt::{self, Display};
use std::rc::Rc;

use varn_core::ModuleId;
use varn_types::FunctionProto;

#[derive(Debug)]
pub struct ModuleError(pub String);

impl ModuleError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Runtime-side adapter over the canonical module system.
///
/// Loading a module's SOURCE and RESOLVING its specifier belong to
/// `varn_modules::loader::ModuleLoader` (ADR-0011); this trait exists only
/// because the VM needs the compiled `FunctionProto` (and native handles) that
/// the loader deliberately does not produce. Implementations must obtain the
/// source through the canonical registry (`FileLoader`/`StdlibLoader` do) and
/// compile it; they must not re-resolve or re-read on their own.
pub trait ModuleLoader {
    fn resolve(&self, specifier: &str, from: &ModuleId) -> Result<ModuleId, ModuleError>;
    fn load(&self, id: &ModuleId) -> Result<Option<Rc<FunctionProto>>, ModuleError>;
    fn native(&self, id: &ModuleId) -> Option<varn_types::Value>;
}

pub struct CompositeLoader {
    loaders: Vec<Box<dyn ModuleLoader + Send + Sync>>,
}

impl CompositeLoader {
    pub fn new(loaders: Vec<Box<dyn ModuleLoader + Send + Sync>>) -> Self {
        Self { loaders }
    }
}

impl ModuleLoader for CompositeLoader {
    fn resolve(&self, specifier: &str, from: &ModuleId) -> Result<ModuleId, ModuleError> {
        let mut last_err = ModuleError::new(format!("cannot resolve '{specifier}'"));
        for loader in &self.loaders {
            match loader.resolve(specifier, from) {
                Ok(id) => return Ok(id),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    fn load(&self, id: &ModuleId) -> Result<Option<Rc<FunctionProto>>, ModuleError> {
        for loader in &self.loaders {
            match loader.load(id) {
                Ok(Some(proto)) => return Ok(Some(proto)),
                Ok(None) => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }

    fn native(&self, id: &ModuleId) -> Option<varn_types::Value> {
        for loader in &self.loaders {
            if let Some(v) = loader.native(id) {
                return Some(v);
            }
        }
        None
    }
}
