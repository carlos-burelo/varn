use std::fmt::{self, Display};
use std::rc::Rc;
use std::sync::Arc;

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

pub trait ModuleLoader {
    fn resolve(&self, specifier: &str, from: &ModuleId) -> Result<ModuleId, ModuleError>;
    fn load(&self, id: &ModuleId) -> Result<Option<Rc<FunctionProto>>, ModuleError>;
    fn native(&self, id: &ModuleId) -> Option<varn_types::Value>;
}

pub struct CompositeLoader {
    loaders: Vec<Box<dyn ModuleLoader>>,
}

impl CompositeLoader {
    pub fn new(loaders: Vec<Box<dyn ModuleLoader>>) -> Self {
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
