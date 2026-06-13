//! Process-wide cache of compiled module bytecode, shared across threads.
//!
//! `FunctionProto` is full of `Rc` and therefore not `Send`, so the cache
//! stores postcard-serialized bytes (which are `Send + Sync`). Each thread
//! that hits the cache deserializes its own private proto graph — much
//! cheaper than re-running lex/parse/check/compile. The main consumer is
//! isolate spawning, where every worker thread previously recompiled the
//! whole stdlib wrapper chain plus the user module from source.

use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use varn_types::FunctionProto;

static CACHE: OnceLock<Mutex<rustc_hash::FxHashMap<String, std::sync::Arc<[u8]>>>> =
    OnceLock::new();

fn cache() -> &'static Mutex<rustc_hash::FxHashMap<String, std::sync::Arc<[u8]>>> {
    CACHE.get_or_init(|| Mutex::new(rustc_hash::FxHashMap::default()))
}

/// Fetch and deserialize a cached proto. Returns `None` on miss or on a
/// deserialization failure (the caller then falls back to compiling).
pub fn get(key: &str) -> Option<Rc<FunctionProto>> {
    let bytes = cache().lock().ok()?.get(key).cloned()?;
    postcard::from_bytes::<FunctionProto>(&bytes).ok().map(Rc::new)
}

pub fn contains(key: &str) -> bool {
    cache().lock().map(|m| m.contains_key(key)).unwrap_or(false)
}

/// Serialize and store a proto. Serialization failures are ignored — the
/// cache is purely an optimization.
pub fn put(key: &str, proto: &FunctionProto) {
    if let Ok(bytes) = postcard::to_allocvec(proto) {
        if let Ok(mut map) = cache().lock() {
            map.insert(key.to_owned(), bytes.into());
        }
    }
}

pub fn put_if_absent(key: &str, proto: &FunctionProto) {
    if !contains(key) {
        put(key, proto);
    }
}
