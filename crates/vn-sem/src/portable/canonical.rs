use rustc_hash::FxHashMap;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

pub(crate) fn sorted<K, V, S>(map: &FxHashMap<K, V>, s: S) -> Result<S::Ok, S::Error>
where
    K: Ord + Serialize,
    V: Serialize,
    S: Serializer,
{
    map.iter().collect::<BTreeMap<_, _>>().serialize(s)
}

pub(crate) fn sorted_nested<K, K2, V, S>(
    map: &FxHashMap<K, FxHashMap<K2, V>>,
    s: S,
) -> Result<S::Ok, S::Error>
where
    K: Ord + Serialize,
    K2: Ord + Serialize,
    V: Serialize,
    S: Serializer,
{
    map.iter()
        .map(|(k, inner)| (k, inner.iter().collect::<BTreeMap<_, _>>()))
        .collect::<BTreeMap<_, _>>()
        .serialize(s)
}
