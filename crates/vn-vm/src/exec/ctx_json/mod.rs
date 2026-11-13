use crate::exec::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::Deserializer;
use std::borrow::Cow;
use vn_types::NativeCtx;

use fast::fast_parse_json;
use serde_parse::VmVisitor;
use stringify::{value_estimate_capacity, write_json_vm};

impl ExecCtx {
    pub(crate) fn json_parse(&mut self, text: &str) -> Result<VmValue, String> {
        if let Some(val) = fast_parse_json(self, text) {
            return Ok(val);
        }
        let mut deserializer = serde_json::Deserializer::from_str(text);
        deserializer
            .deserialize_any(VmVisitor(self))
            .map_err(|e| format!("JSON.parse: {e}"))
    }

    pub(crate) fn json_stringify(&self, value: VmValue) -> Result<String, String> {
        let mut out = String::with_capacity(value_estimate_capacity(self, value));
        write_json_vm(self, value, &mut out, 0);
        if let Some(fault) = stringify::take_fault() {
            return Err(format!("JSON.stringify: {fault}"));
        }
        Ok(out)
    }
}

mod fast;
mod number;
mod serde_parse;
mod stringify;

pub(crate) struct CacheEntry {
    pub(super) keys: std::rc::Rc<Vec<String>>,
    pub(super) shape: std::rc::Rc<vn_types::Shape>,
    pub(super) tokens: Vec<Option<Box<[u8]>>>,
}

impl CacheEntry {
    pub(super) fn new(keys: Vec<String>, shape: std::rc::Rc<vn_types::Shape>) -> Self {
        let tokens = keys
            .iter()
            .map(|key| {
                let plain = key.bytes().all(|b| b >= 0x20 && b != b'"' && b != b'\\');
                plain.then(|| format!("\"{key}\":").into_bytes().into_boxed_slice())
            })
            .collect();
        CacheEntry {
            keys: std::rc::Rc::new(keys),
            shape,
            tokens,
        }
    }
}

pub(super) const JSON_SHAPE_CAP: usize = 64;
