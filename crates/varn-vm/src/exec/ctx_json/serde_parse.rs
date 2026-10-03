use super::*;

pub(super) struct VmSeed<'a>(pub(super) &'a mut ExecCtx);

impl<'de, 'a> DeserializeSeed<'de> for VmSeed<'a> {
    type Value = VmValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(VmVisitor(self.0))
    }
}

pub(super) struct VmVisitor<'a>(pub(super) &'a mut ExecCtx);

impl<'de, 'a> Visitor<'de> for VmVisitor<'a> {
    type Value = VmValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("any valid JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
        Ok(VmValue::from_bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
        Ok(VmValue::from_int(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
        if v <= i64::MAX as u64 {
            Ok(VmValue::from_int(v as i64))
        } else {
            Ok(VmValue::from_f64(v as f64))
        }
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
        Ok(VmValue::from_f64(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok(self.0.heap.alloc_str_dynamic(v))
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok(self.0.heap.alloc_str_dynamic(v))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok(self.0.heap.alloc_str_dynamic(&v))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(VmValue::null())
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(VmValue::null())
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(elem) = seq.next_element_seed(VmSeed(self.0))? {
            items.push(elem);
        }
        Ok(self.0.alloc_array(items))
    }

    /// Reads an object's fields straight into a fixed buffer, checking each key
    /// against the cached shape as it arrives.
    ///
    /// This used to collect keys and values into two `Vec`s, then compare the
    /// whole key list against the cache — two throwaway allocations per object,
    /// plus a third inside `alloc_object_with_shape`, which copies the values
    /// into the object's inline storage and drops the `Vec` again. A document
    /// of 50 000 objects paid that 50 000 times per parse.
    ///
    /// Matching incrementally means a hit never materialises the keys at all:
    /// the keys that matched ARE the cached ones. Only a mismatch has to
    /// recover them, and it recovers the matched prefix from the cache rather
    /// than from the parse.
    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        /// Fields held without allocating. Objects wider than this fall back to
        /// the growable path; JSON documents in the shape this matters for
        /// (records in an array) are far narrower.
        const INLINE_FIELDS: usize = 16;

        let mut inline = [VmValue::null(); INLINE_FIELDS];
        let mut spilled: Vec<VmValue> = Vec::new();
        let mut n = 0usize;

        // Taken once: the loop below parses nested values, which can replace
        // the cache underneath it.
        let cached = cache_snapshot();
        let cached_keys = |k: usize| cached.as_ref().and_then(|(keys, _)| keys.get(k));

        // How many keys so far are the snapshot's, in order. `None` once a key
        // has diverged — from then on keys are collected as owned strings.
        let mut matched: Option<usize> = Some(0);
        let mut owned_keys: Vec<String> = Vec::new();

        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            let val = map.next_value_seed(VmSeed(self.0))?;
            if n < INLINE_FIELDS {
                inline[n] = val;
            } else {
                if spilled.is_empty() {
                    spilled.extend_from_slice(&inline);
                }
                spilled.push(val);
            }
            n += 1;

            match matched {
                Some(k) if cached_keys(k).map(String::as_str) == Some(key.as_ref()) => {
                    matched = Some(k + 1);
                }
                Some(k) => {
                    // Diverged at `k`: the first `k` keys are the snapshot's.
                    owned_keys = key_prefix(&cached, k);
                    owned_keys.push(key.into_owned());
                    matched = None;
                }
                None => owned_keys.push(key.into_owned()),
            }
        }

        let values: &[VmValue] = if spilled.is_empty() {
            &inline[..n]
        } else {
            &spilled
        };

        // A prefix match is not a match: the object must also have ENDED where
        // the snapshot's key list does.
        if matched == Some(n) {
            if let Some((keys, shape)) = &cached {
                if keys.len() == n {
                    return Ok(self.0.heap.alloc_object_with_shape_slice(shape, values));
                }
            }
        }

        // No cached shape, or this object has a different one: build the object
        // field by field so the shape is derived, then cache it for the objects
        // that follow.
        if let Some(k) = matched {
            owned_keys = key_prefix(&cached, k);
        }
        let obj = self.0.alloc_object();
        for (k, v) in owned_keys.iter().zip(values.iter()) {
            self.0.set_field(obj, k, *v);
        }
        if let Some(shape) = self.0.get_object_shape(obj) {
            let entry = (std::rc::Rc::new(owned_keys), shape);
            JSON_SHAPE_CACHE.with(|c| *c.borrow_mut() = Some(entry));
        }
        Ok(obj)
    }
}
