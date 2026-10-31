pub use varn_types::chunk::FunctionProto;

pub mod from_tir;
pub mod hir;
pub mod lower;
pub mod passes;
pub mod regalloc;
pub mod ssa;

#[derive(Debug)]
pub enum OptError {
    Unsupported(&'static str),
    InvalidTir(Vec<String>),
    Internal(String),
    OutOfRegisters { function: String, needed: usize },
}

pub const MAX_REGISTERS: usize = 256;

impl OptError {
    pub(crate) fn in_function(self, name: &str) -> Self {
        match self {
            Self::OutOfRegisters { needed, .. } => Self::OutOfRegisters {
                function: name.to_owned(),
                needed,
            },
            other @ (Self::Unsupported(_) | Self::InvalidTir(_) | Self::Internal(_)) => other,
        }
    }
}

impl std::fmt::Display for OptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "unsupported construct: {what}"),
            Self::InvalidTir(problems) => write!(f, "invalid TIR: {}", problems.join("; ")),
            Self::Internal(what) => write!(f, "internal compiler error: {what}"),
            Self::OutOfRegisters { function, needed } => write!(
                f,
                "function `{function}` needs {needed} registers but a function has at most {MAX_REGISTERS}: split it into smaller functions or take fewer parameters"
            ),
        }
    }
}
