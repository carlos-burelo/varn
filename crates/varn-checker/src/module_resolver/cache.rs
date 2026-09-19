use crate::binder::{BindResult, Extensions, TypeMembers};
use crate::scope::{ScopeArena, ScopeId};
use crate::symbol::{CacheableSymbol, Symbol, SymbolArena};
use crate::types::Type;
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;
use varn_core::AtomInterner;

pub type ExportMap = FxHashMap<String, crate::symbol::Symbol>;

/// Not stored directly: [`CachedModule`] holds the on-disk twin
/// ([`CacheableModule`]) instead, and this is only what callers get back
/// after re-interning against the current session.
pub(super) struct CachedModule {
    pub exports: ExportMap,
    pub bind: BindResult,
}

/// On-disk form of [`CachedModule`]: every `Atom`-bearing piece (`Symbol`
/// inside `exports`, `Symbol` inside `bind.arena`) resolved to text — see
/// [`CacheableSymbol`] for why a raw `Atom` cannot cross this boundary.
///
/// The other `BindResult` fields (`class_methods`, `type_members`, ...) are
/// already keyed by `Rc<str>`/text, so they serialize as-is; `BindResult`'s
/// own `#[serde(skip)]` fields (`diagnostics`, `interner`, `core`,
/// `pending_enrich`, `evolved_array_types`) are in-process-only and are
/// rebuilt to their defaults on load, same as they already are when
/// `BindResult` itself is (de)serialized directly.
#[derive(serde::Serialize, serde::Deserialize)]
struct CacheableModule {
    exports: FxHashMap<String, CacheableSymbol>,
    arena: Vec<CacheableSymbol>,
    scopes: ScopeArena,
    global_scope: ScopeId,
    class_methods: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
    type_members: TypeMembers,
    class_parents: FxHashMap<Rc<str>, Rc<str>>,
    source_file: Rc<str>,
    sum_type_variants: FxHashMap<Rc<str>, Vec<Rc<str>>>,
    sum_variant_parent: FxHashMap<Rc<str>, Rc<str>>,
    sum_variant_fields: FxHashMap<Rc<str>, Vec<(Rc<str>, Type)>>,
    extensions: Extensions,
}

impl CacheableModule {
    /// Resolve every `Atom` in `exports`/`bind` against `interner` — the
    /// `AtomInterner` of the session that produced them — into the owned
    /// form that goes to disk.
    fn from_live(exports: &ExportMap, bind: &BindResult, interner: &AtomInterner) -> Self {
        Self {
            exports: exports
                .iter()
                .map(|(k, v)| (k.clone(), v.to_cacheable(interner)))
                .collect(),
            arena: bind
                .arena
                .all()
                .iter()
                .map(|s| s.to_cacheable(interner))
                .collect(),
            scopes: bind.scopes.clone(),
            global_scope: bind.global_scope,
            class_methods: bind.class_methods.clone(),
            type_members: bind.type_members.clone(),
            class_parents: bind.class_parents.clone(),
            source_file: bind.source_file.clone(),
            sum_type_variants: bind.sum_type_variants.clone(),
            sum_variant_parent: bind.sum_variant_parent.clone(),
            sum_variant_fields: bind.sum_variant_fields.clone(),
            extensions: bind.extensions.clone(),
        }
    }

    /// Re-intern every text field against `interner` — the current session's
    /// `AtomInterner`, distinct from whatever produced this cache entry — and
    /// rebuild the scope-lookup tables (`CheckerScope::bindings`, itself
    /// `#[serde(skip)]` for the same "raw `Atom` means nothing across
    /// sessions" reason) from `arena` now that its names are valid `Atom`s
    /// again.
    fn into_live(self, interner: &mut AtomInterner) -> (ExportMap, BindResult) {
        let exports = self
            .exports
            .into_iter()
            .map(|(k, v)| (k, Symbol::from_cacheable(v, interner)))
            .collect();
        let mut arena = SymbolArena::default();
        for c in self.arena {
            arena.push(Symbol::from_cacheable(c, interner));
        }
        let mut scopes = self.scopes;
        rebuild_scope_bindings(&mut scopes, &arena);
        let bind = BindResult {
            arena,
            scopes,
            global_scope: self.global_scope,
            diagnostics: varn_core::DiagnosticBag::default(),
            interner: interner.clone(),
            // KNOWN CAVEAT (see `CheckerTyId`'s serde derive doc in
            // `types/interned.rs`): a bare `CheckerTyId` round-trips a
            // number, not a type, without the table that produced it, and
            // this cache format carries no such table. A fresh empty table
            // is the honest placeholder — no `Type` reached through this
            // `BindResult`'s cached fields (`class_methods`, `type_members`,
            // `sum_variant_fields`, ...) should be treated as resolvable
            // against it. Fixing this for real is out of this task's scope,
            // same as when this caveat was first documented.
            ty_table: crate::types::CheckerTyTable::default(),
            class_methods: self.class_methods,
            type_members: self.type_members,
            class_parents: self.class_parents,
            source_file: self.source_file,
            sum_type_variants: self.sum_type_variants,
            sum_variant_parent: self.sum_variant_parent,
            sum_variant_fields: self.sum_variant_fields,
            extensions: self.extensions,
            core: None,
            pending_enrich: Vec::new(),
            evolved_array_types: FxHashMap::default(),
        };
        (exports, bind)
    }
}

/// `CheckerScope::bindings` is `#[serde(skip)]` (an `Atom`-keyed map, same
/// rationale as everywhere else in this file) so it deserializes as empty.
/// Every entry it should hold is recoverable from `ordered` plus the
/// now-re-interned `arena`, so rebuild it instead of caching it.
fn rebuild_scope_bindings(scopes: &mut ScopeArena, arena: &SymbolArena) {
    for scope_id in 0.. {
        if scope_id >= scopes.len() {
            break;
        }
        let scope = scopes.get_mut(scope_id);
        let ordered = scope.ordered.clone();
        for id in ordered {
            let name = arena.get(id).name;
            scope.insert_binding_only(name, id);
        }
    }
}

pub fn serialize_module_interface(
    exports: &ExportMap,
    bind: &BindResult,
    interner: &AtomInterner,
) -> Result<Vec<u8>, String> {
    let cached = CacheableModule::from_live(exports, bind, interner);
    postcard::to_allocvec(&cached).map_err(|e| e.to_string())
}

pub fn deserialize_module_interface(
    bytes: &[u8],
    interner: &mut AtomInterner,
) -> Result<(ExportMap, BindResult), String> {
    let cached: CacheableModule = postcard::from_bytes(bytes).map_err(|e| e.to_string())?;
    let (mut exports, bind) = cached.into_live(interner);
    super::exports::assign_slots(&mut exports);
    Ok((exports, bind))
}

pub(super) fn compute_source_hash(source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn get_cache_dir(resolver: &super::DiskResolver) -> PathBuf {
    resolver.types_cache_dir()
}

pub(super) fn try_load_cache(
    resolver: &super::DiskResolver,
    virtual_id: &str,
    source: &str,
) -> Option<CachedModule> {
    if virtual_id == "core:types" {
        return None;
    }
    let hash = compute_source_hash(source);
    let name = if virtual_id.contains(':') {
        virtual_id.replace(':', "_")
    } else {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        virtual_id.hash(&mut hasher);
        format!("file_{:x}", hasher.finish())
    };

    let cache_dir = get_cache_dir(resolver);
    let cache_file = cache_dir.join(format!(
        "{}.{:x}.{:08x}.vnm",
        name,
        hash,
        varn_modules::artifact::cache_key()
    ));
    if !cache_file.exists() {
        return None;
    }
    let bytes = std::fs::read(&cache_file).ok()?;
    let payload = match varn_modules::artifact::read_artifact(
        varn_modules::artifact::ArtifactKind::CheckerInterface,
        &bytes,
    ) {
        Ok(p) => p,
        Err(_) => return None,
    };
    // Re-intern against this compilation's own interner (not whatever
    // process wrote this cache file, possibly a session ago and always a
    // different `AtomInterner`) and publish the grown table back, same
    // pattern as `parse_and_cache`.
    let mut interner = resolver.interner_snapshot();
    let result = deserialize_module_interface(payload, &mut interner);
    resolver.set_interner(interner);
    match result {
        Ok((exports, bind)) => Some(CachedModule { exports, bind }),
        Err(_) => None,
    }
}

pub(super) fn save_to_cache(
    resolver: &super::DiskResolver,
    virtual_id: &str,
    source: &str,
    exports: &ExportMap,
    bind: &BindResult,
) {
    if virtual_id == "core:types" {
        return;
    }
    let hash = compute_source_hash(source);
    let cache_dir = get_cache_dir(resolver);
    let _ = std::fs::create_dir_all(&cache_dir);
    let name = if virtual_id.contains(':') {
        virtual_id.replace(':', "_")
    } else {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        virtual_id.hash(&mut hasher);
        format!("file_{:x}", hasher.finish())
    };

    let cache_file = cache_dir.join(format!(
        "{}.{:x}.{:08x}.vnm",
        name,
        hash,
        varn_modules::artifact::cache_key()
    ));
    let interner = resolver.interner_snapshot();
    if let Ok(payload) = serialize_module_interface(exports, bind, &interner) {
        let bytes = varn_modules::artifact::write_artifact(
            varn_modules::artifact::ArtifactKind::CheckerInterface,
            varn_modules::artifact::ArtifactClass::Cache,
            &payload,
        );
        let _ = varn_modules::artifact::write_artifact_file(&cache_file, &bytes);
        varn_modules::artifact::prune_superseded(&cache_file);
    }
}
