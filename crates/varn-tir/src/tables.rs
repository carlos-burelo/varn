//! Per-module tables. Populated in the next task.

use crate::ty::BackendTy;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub name: Rc<str>,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub name: Rc<str>,
}

#[derive(Debug, Clone)]
pub struct Signature {
    pub params: Vec<BackendTy>,
    pub return_ty: BackendTy,
}
