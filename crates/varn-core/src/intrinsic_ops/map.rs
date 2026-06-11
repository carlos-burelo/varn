use std::collections::HashMap;
use std::sync::OnceLock;

use super::{array, math, string, types};

static INTRINSIC_MAP: OnceLock<HashMap<&'static str, u8>> = OnceLock::new();

fn map() -> &'static HashMap<&'static str, u8> {
    INTRINSIC_MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for &(key, val) in math::MAP_ENTRIES
            .iter()
            .chain(string::MAP_ENTRIES.iter())
            .chain(array::MAP_ENTRIES.iter())
            .chain(types::MAP_ENTRIES.iter())
        {
            m.insert(key, val);
        }
        m
    })
}

pub fn lookup(binding_key: &str) -> Option<u8> {
    map().get(binding_key).copied()
}
