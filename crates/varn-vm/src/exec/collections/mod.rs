use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use std::sync::Arc;
use varn_types::value::ObjRef;

mod array_ops;
mod build;
mod index;
mod object_ops;

pub(crate) use array_ops::*;
pub(crate) use build::*;
pub(crate) use index::*;
pub(crate) use object_ops::*;
