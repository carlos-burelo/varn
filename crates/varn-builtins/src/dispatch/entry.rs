use crate::runtime_ops::RuntimeOpFn;

#[derive(Clone, Copy)]
pub struct DispatchEntry {
    pub id: u64,
    pub module_id: &'static str,
    pub name: &'static str,
    pub func: RuntimeOpFn,
    pub capability: Option<&'static str>,
}

pub use varn_core::op_id::{compound_op_id, compound_op_id3};
