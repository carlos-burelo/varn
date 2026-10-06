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
}
