use crate::exec::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::Deserializer;
use std::borrow::Cow;
use varn_types::NativeCtx;

use fast::fast_parse_json;
use serde_parse::VmVisitor;
use stringify::{value_estimate_capacity, write_json_vm};

thread_local! {
    static JSON_SHAPE_CACHE: ShapeCache = const { std::cell::RefCell::new(None) };
}

impl ExecCtx {
    pub(crate) fn json_parse(&mut self, text: &str) -> Result<VmValue, String> {
        JSON_SHAPE_CACHE.with(|c| *c.borrow_mut() = None);
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
        write_json_vm(self, value, &mut out);
        Ok(out)
    }
}

mod fast;
mod number;
mod serde_parse;
mod stringify;

pub(super) type CacheEntry = (std::rc::Rc<Vec<String>>, std::rc::Rc<varn_types::Shape>);
pub(super) type ShapeCache = std::cell::RefCell<Option<CacheEntry>>;

pub(super) fn cache_snapshot() -> Option<CacheEntry> {
    JSON_SHAPE_CACHE.with(|c| c.borrow().clone())
}

pub(super) fn key_prefix(cached: &Option<CacheEntry>, n: usize) -> Vec<String> {
    match cached {
        Some((keys, _)) => keys.iter().take(n).cloned().collect(),
        None => Vec::new(),
    }
}
