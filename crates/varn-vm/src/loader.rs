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

pub trait ModuleLoader {
    fn resolve(&self, specifier: &str, from: &ModuleId) -> Result<ModuleId, ModuleError>;
    fn load(&self, id: &ModuleId) -> Result<Option<Rc<FunctionProto>>, ModuleError>;
}
