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

/// The last object shape a parse produced, with the key list it came from.
///
/// Both halves are behind `Rc` so an object can take a SNAPSHOT of the pair
/// for the duration of its own `visit_map` — two refcount bumps — instead of
/// reaching through the thread-local and the `RefCell` once per field.
///
/// Taking it once also closes a hazard rather than just saving work: the
/// previous code re-read the thread-local per key AND once more at the end, so
/// the entry it finally used need not be the one it compared the keys against
/// — a nested object parsed mid-loop replaces the cache. **This is not a
/// demonstrated bug**; the obvious reproducer (a nested object of the same
/// arity between two siblings, pinned in `tests/73-str-charcode-json-shape.vn`)
/// produces correct output on the previous code too. It is removed because a
/// match against one entry and a build from another is a coincidence to rely
/// on, not an invariant.
pub(super) type CacheEntry = (std::rc::Rc<Vec<String>>, std::rc::Rc<varn_types::Shape>);
pub(super) type ShapeCache = std::cell::RefCell<Option<CacheEntry>>;

/// The current cache entry, if any. Cheap enough to call once per object.
pub(super) fn cache_snapshot() -> Option<CacheEntry> {
    JSON_SHAPE_CACHE.with(|c| c.borrow().clone())
}

/// The snapshot's first `n` keys, owned — how an object that matched up to a
/// point and then diverged recovers the names it never had to materialise.
pub(super) fn key_prefix(cached: &Option<CacheEntry>, n: usize) -> Vec<String> {
    match cached {
        Some((keys, _)) => keys.iter().take(n).cloned().collect(),
        None => Vec::new(),
    }
}
