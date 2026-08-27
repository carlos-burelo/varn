use rustc_hash::FxHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportedSymbol {
    pub name: String,
    pub kind_str: String,
    pub signature_str: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleExports {
    pub exports: BTreeMap<String, ExportedSymbol>,
    pub fingerprint: u64,
}

impl ModuleExports {
    pub fn build(exports: Vec<ExportedSymbol>) -> Self {
        let mut map = BTreeMap::new();
        let mut hasher = FxHasher::default();

        for sym in exports {
            sym.name.hash(&mut hasher);
            sym.kind_str.hash(&mut hasher);
            sym.signature_str.hash(&mut hasher);
            map.insert(sym.name.clone(), sym);
        }

        let fingerprint = hasher.finish();

        Self {
            exports: map,
            fingerprint,
        }
    }

    /// Firewall check: returns true if public exports remain identical
    pub fn is_unchanged_from(&self, other: &Self) -> bool {
        self.fingerprint == other.fingerprint && self.exports == other.exports
    }
}
