use rustc_hash::FxHashMap as HashMap;
use std::sync::OnceLock;

use super::math;

static INTRINSIC_MAP: OnceLock<HashMap<&'static str, u8>> = OnceLock::new();

fn map() -> &'static HashMap<&'static str, u8> {
    INTRINSIC_MAP.get_or_init(|| {
        let mut m = HashMap::default();
        for &(key, val) in math::MAP_ENTRIES.iter() {
            m.insert(key, val);
        }
        m
    })
}

pub fn lookup(binding_key: &str) -> Option<u8> {
    map().get(binding_key).copied()
}
