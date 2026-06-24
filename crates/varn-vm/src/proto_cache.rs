








use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use varn_types::FunctionProto;

static CACHE: OnceLock<Mutex<rustc_hash::FxHashMap<String, std::sync::Arc<[u8]>>>> =
    OnceLock::new();

fn cache() -> &'static Mutex<rustc_hash::FxHashMap<String, std::sync::Arc<[u8]>>> {
    CACHE.get_or_init(|| Mutex::new(rustc_hash::FxHashMap::default()))
}



pub fn get(key: &str) -> Option<Rc<FunctionProto>> {
    let bytes = cache().lock().ok()?.get(key).cloned()?;
    postcard::from_bytes::<FunctionProto>(&bytes).ok().map(Rc::new)
}

pub fn contains(key: &str) -> bool {
    cache().lock().map(|m| m.contains_key(key)).unwrap_or(false)
}



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
