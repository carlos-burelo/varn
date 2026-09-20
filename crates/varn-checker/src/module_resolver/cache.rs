//! Formato en disco de la interfaz de un módulo.
//!
//! Nada de lo que cruza esta frontera puede llevar un `CheckerTyId`: un `Type`
//! solo significa algo en la `CheckerTyTable` que lo internó, y el proceso que
//! escribe el caché no es el que lo lee. Por eso todo tipo viaja como
//! [`PortableType`] (nombres + estructura, sin ids) y se re-interna en la tabla
//! del lector. Ver `types/portable.rs` y `AGENTS.md` §§0–2.

use crate::binder::{BindResult, Extensions, TypeMembers};
use crate::module_resolver::ImportResolver;
use crate::scope::{ScopeArena, ScopeId};
use crate::symbol::{Symbol, SymbolArena};
use crate::types::{
    decode_portable_type, encode_portable_type, CheckerTyTable, ClassMemberInfo, ClassMemberKind,
    PortableType, Type,
};
use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::sync::Arc;
use varn_core::ast::operators::Visibility;
use varn_core::{Atom, AtomInterner, TypeTag};

pub type ExportMap = FxHashMap<String, Symbol>;

/// Not stored directly: [`CachedModule`] holds the on-disk twin
/// ([`PortableModule`]) instead, and this is only what callers get back after
/// re-interning against the current session.
pub(super) struct CachedModule {
    pub exports: ExportMap,
    pub bind: BindResult,
}

// ── Espejo portable de `Symbol` ───────────────────────────────────────────

/// On-disk form of a `Symbol`: every `Atom` resolved to text and every `Type`
/// encoded as a [`PortableType`]. It is also the in-memory crossing used by
/// `binder/imports.rs`, so there is exactly one way a symbol crosses a module
/// boundary — text for `Atom`, owner-table shape for `Type`, both re-interned
/// into the reader.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct PortableSymbol {
    kind: crate::symbol::SymbolKind,
    name: String,
    ty: Option<PortableType>,
    line: u32,
    col: u32,
    has_explicit_type: bool,
    is_async: bool,
    is_generator: bool,
    doc: Option<String>,
    type_params: Vec<String>,
    type_param_constraints: Vec<Option<PortableType>>,
    offset: u32,
    origin_module: Option<String>,
    re_export_path: Vec<String>,
    original_name: Option<String>,
    slot_idx: Option<usize>,
    intrinsic_wire: Option<u8>,
}

/// Codifica `ty` contra la tabla que realmente lo internó: la del bind si el id
/// está en rango, si no la del módulo que lo declara (`origin`), y como último
/// recurso degrada a `Dynamic` (nunca indexar fuera de rango).
///
/// Necesario para los símbolos RE-EXPORTADOS: su `ty` pertenece al módulo
/// declarante, no al que re-exporta, y por eso puede estar fuera de
/// `bind.ty_table`.
fn encode_with_owner(
    ty: Type,
    origin: Option<Atom>,
    fallback_table: &CheckerTyTable,
    resolver: Option<&dyn ImportResolver>,
    interner: &AtomInterner,
) -> PortableType {
    // Preferir la tabla del módulo DECLARANTE: un símbolo re-exportado lleva
    // ids de ese módulo, y su número puede caer dentro del rango del bind que
    // re-exporta con otro significado.
    if let (Some(module), Some(resolver)) = (origin.and_then(|a| interner.try_resolve(a)), resolver)
    {
        if let Some(b) = resolver
            .stdlib_bind(module)
            .or_else(|| resolver.module_bind(module))
        {
            if b.ty_table.contains(ty.0) {
                return encode_portable_type(ty, &b.ty_table, interner);
            }
        }
    }
    if fallback_table.contains(ty.0) {
        return encode_portable_type(ty, fallback_table, interner);
    }
    PortableType::Intrinsic(TypeTag::Dynamic)
}

pub(crate) fn encode_symbol(
    s: &Symbol,
    bind_table: &CheckerTyTable,
    interner: &AtomInterner,
    resolver: Option<&dyn ImportResolver>,
) -> PortableSymbol {
    PortableSymbol {
        kind: s.kind,
        name: interner.resolve(s.name).to_string(),
        ty: s
            .ty
            .map(|t| encode_with_owner(t, s.origin_module, bind_table, resolver, interner)),
        line: s.line,
        col: s.col,
        has_explicit_type: s.has_explicit_type,
        is_async: s.is_async,
        is_generator: s.is_generator,
        doc: s.doc.map(|a| interner.resolve(a).to_string()),
        type_params: s
            .type_params
            .iter()
            .map(|a| interner.resolve(*a).to_string())
            .collect(),
        type_param_constraints: s
            .type_param_constraints
            .iter()
            .map(|c| {
                c.map(|t| encode_with_owner(t, s.origin_module, bind_table, resolver, interner))
            })
            .collect(),
        offset: s.offset,
        origin_module: s.origin_module.map(|a| interner.resolve(a).to_string()),
        re_export_path: s
            .re_export_path
            .iter()
            .map(|a| interner.resolve(*a).to_string())
            .collect(),
        original_name: s.original_name.map(|a| interner.resolve(a).to_string()),
        slot_idx: s.slot_idx,
        intrinsic_wire: s.intrinsic_wire,
    }
}

pub(crate) fn decode_symbol(
    p: PortableSymbol,
    table: &mut CheckerTyTable,
    interner: &mut AtomInterner,
) -> Symbol {
    let ty =
        p.ty.as_ref()
            .map(|t| decode_portable_type(t, table, interner));
    let constraints = p
        .type_param_constraints
        .iter()
        .map(|c| c.as_ref().map(|t| decode_portable_type(t, table, interner)))
        .collect();
    Symbol {
        kind: p.kind,
        name: interner.intern(&p.name),
        ty,
        line: p.line,
        col: p.col,
        has_explicit_type: p.has_explicit_type,
        is_async: p.is_async,
        is_generator: p.is_generator,
        doc: p.doc.map(|s| interner.intern(&s)),
        type_params: p.type_params.iter().map(|s| interner.intern(s)).collect(),
        type_param_constraints: constraints,
        offset: p.offset,
        full_range: varn_core::SourceRange::default(),
        origin_module: p.origin_module.map(|s| interner.intern(&s)),
        re_export_path: p
            .re_export_path
            .iter()
            .map(|s| interner.intern(s))
            .collect(),
        original_name: p.original_name.map(|s| interner.intern(&s)),
        alias_node: None,
        slot_idx: p.slot_idx,
        intrinsic_wire: p.intrinsic_wire,
    }
}

// ── Espejo portable de `ClassMemberInfo` / `TypeMembers` ──────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct PortableClassMemberInfo {
    name: Arc<str>,
    kind: ClassMemberKind,
    is_async: bool,
    is_generator: bool,
    is_static: bool,
    is_optional: bool,
    line: u32,
    col: u32,
    offset: u32,
    ty: PortableType,
    members: Vec<PortableClassMemberInfo>,
    visibility: Option<Visibility>,
    is_abstract: bool,
    is_readonly: bool,
    is_override: bool,
    is_builtin_or_intrinsic: bool,
    symbol_id: Option<usize>,
}

fn encode_member(
    m: &ClassMemberInfo,
    table: &CheckerTyTable,
    interner: &AtomInterner,
) -> PortableClassMemberInfo {
    PortableClassMemberInfo {
        name: m.name.clone(),
        kind: m.kind,
        is_async: m.is_async,
        is_generator: m.is_generator,
        is_static: m.is_static,
        is_optional: m.is_optional,
        line: m.line,
        col: m.col,
        offset: m.offset,
        ty: encode_portable_type(m.ty, table, interner),
        members: m
            .members
            .iter()
            .map(|c| encode_member(c, table, interner))
            .collect(),
        visibility: m.visibility,
        is_abstract: m.is_abstract,
        is_readonly: m.is_readonly,
        is_override: m.is_override,
        is_builtin_or_intrinsic: m.is_builtin_or_intrinsic,
        symbol_id: m.symbol_id,
    }
}

fn decode_member(
    p: &PortableClassMemberInfo,
    table: &mut CheckerTyTable,
    interner: &mut AtomInterner,
) -> ClassMemberInfo {
    ClassMemberInfo {
        name: p.name.clone(),
        kind: p.kind,
        is_async: p.is_async,
        is_generator: p.is_generator,
        is_static: p.is_static,
        is_optional: p.is_optional,
        line: p.line,
        col: p.col,
        offset: p.offset,
        ty: decode_portable_type(&p.ty, table, interner),
        members: p
            .members
            .iter()
            .map(|c| decode_member(c, table, interner))
            .collect(),
        visibility: p.visibility,
        is_abstract: p.is_abstract,
        is_readonly: p.is_readonly,
        is_override: p.is_override,
        is_builtin_or_intrinsic: p.is_builtin_or_intrinsic,
        symbol_id: p.symbol_id,
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct PortableTypeMembers {
    classes: FxHashMap<Arc<str>, PortableClassMemberInfo>,
    interfaces: FxHashMap<Arc<str>, Vec<PortableClassMemberInfo>>,
    enums: FxHashMap<Arc<str>, Vec<PortableClassMemberInfo>>,
    namespaces: FxHashMap<Arc<str>, Vec<PortableClassMemberInfo>>,
    flattened: FxHashMap<Arc<str>, Vec<PortableClassMemberInfo>>,
    getters: FxHashMap<Arc<str>, FxHashMap<Arc<str>, PortableType>>,
    setters: FxHashMap<Arc<str>, FxHashMap<Arc<str>, PortableType>>,
}

fn encode_member_list(
    list: &[ClassMemberInfo],
    table: &CheckerTyTable,
    interner: &AtomInterner,
) -> Vec<PortableClassMemberInfo> {
    list.iter()
        .map(|m| encode_member(m, table, interner))
        .collect()
}

fn decode_member_list(
    list: &[PortableClassMemberInfo],
    table: &mut CheckerTyTable,
    interner: &mut AtomInterner,
) -> Vec<ClassMemberInfo> {
    list.iter()
        .map(|m| decode_member(m, table, interner))
        .collect()
}

fn encode_type_members(
    tm: &TypeMembers,
    table: &CheckerTyTable,
    interner: &AtomInterner,
) -> PortableTypeMembers {
    let encode_map = |src: &FxHashMap<Arc<str>, Vec<ClassMemberInfo>>| {
        src.iter()
            .map(|(k, v)| (k.clone(), encode_member_list(v, table, interner)))
            .collect()
    };
    let encode_ty_map = |src: &FxHashMap<Arc<str>, FxHashMap<Arc<str>, Type>>| {
        src.iter()
            .map(|(k, inner)| {
                (
                    k.clone(),
                    inner
                        .iter()
                        .map(|(ik, t)| (ik.clone(), encode_portable_type(*t, table, interner)))
                        .collect(),
                )
            })
            .collect()
    };
    PortableTypeMembers {
        classes: tm
            .classes
            .iter()
            .map(|(k, v)| (k.clone(), encode_member(v, table, interner)))
            .collect(),
        interfaces: encode_map(&tm.interfaces),
        enums: encode_map(&tm.enums),
        namespaces: encode_map(&tm.namespaces),
        flattened: encode_map(&tm.flattened),
        getters: encode_ty_map(&tm.getters),
        setters: encode_ty_map(&tm.setters),
    }
}

fn decode_type_members(
    p: &PortableTypeMembers,
    table: &mut CheckerTyTable,
    interner: &mut AtomInterner,
) -> TypeMembers {
    let mut classes = FxHashMap::default();
    for (k, v) in &p.classes {
        classes.insert(k.clone(), decode_member(v, table, interner));
    }
    let mut interfaces = FxHashMap::default();
    for (k, v) in &p.interfaces {
        interfaces.insert(k.clone(), decode_member_list(v, table, interner));
    }
    let mut enums = FxHashMap::default();
    for (k, v) in &p.enums {
        enums.insert(k.clone(), decode_member_list(v, table, interner));
    }
    let mut namespaces = FxHashMap::default();
    for (k, v) in &p.namespaces {
        namespaces.insert(k.clone(), decode_member_list(v, table, interner));
    }
    let mut flattened = FxHashMap::default();
    for (k, v) in &p.flattened {
        flattened.insert(k.clone(), decode_member_list(v, table, interner));
    }
    let mut getters = FxHashMap::default();
    for (k, inner) in &p.getters {
        let mut m = FxHashMap::default();
        for (ik, t) in inner {
            m.insert(ik.clone(), decode_portable_type(t, table, interner));
        }
        getters.insert(k.clone(), m);
    }
    let mut setters = FxHashMap::default();
    for (k, inner) in &p.setters {
        let mut m = FxHashMap::default();
        for (ik, t) in inner {
            m.insert(ik.clone(), decode_portable_type(t, table, interner));
        }
        setters.insert(k.clone(), m);
    }
    TypeMembers {
        classes,
        interfaces,
        objects: FxHashMap::default(),
        enums,
        namespaces,
        flattened,
        getters,
        setters,
    }
}

// ── Módulo completo ───────────────────────────────────────────────────────

/// On-disk form of [`CachedModule`].
///
/// `BindResult`'s in-process-only fields (`diagnostics`, `interner`, `core`,
/// `pending_enrich`, `evolved_array_types`, `type_members.objects`) are
/// `#[serde(skip)]` there too and are rebuilt to their defaults on load.
#[derive(serde::Serialize, serde::Deserialize)]
struct PortableModule {
    exports: FxHashMap<String, PortableSymbol>,
    arena: Vec<PortableSymbol>,
    scopes: ScopeArena,
    global_scope: ScopeId,
    class_methods: FxHashMap<Arc<str>, FxHashMap<Arc<str>, PortableType>>,
    type_members: PortableTypeMembers,
    class_parents: FxHashMap<Arc<str>, Arc<str>>,
    source_file: Arc<str>,
    sum_type_variants: FxHashMap<Arc<str>, Vec<Arc<str>>>,
    sum_variant_parent: FxHashMap<Arc<str>, Arc<str>>,
    sum_variant_fields: FxHashMap<Arc<str>, Vec<(Arc<str>, PortableType)>>,
    extensions: Extensions,
}

impl PortableModule {
    /// Codifica `exports`/`bind` contra su propia tabla e interner.
    fn from_live(
        exports: &ExportMap,
        bind: &BindResult,
        interner: &AtomInterner,
        resolver: Option<&dyn ImportResolver>,
    ) -> Self {
        let table = &bind.ty_table;
        Self {
            exports: exports
                .iter()
                .map(|(k, v)| (k.clone(), encode_symbol(v, table, interner, resolver)))
                .collect(),
            arena: bind
                .arena
                .all()
                .iter()
                .map(|s| encode_symbol(s, table, interner, resolver))
                .collect(),
            scopes: bind.scopes.clone(),
            global_scope: bind.global_scope,
            class_methods: bind
                .class_methods
                .iter()
                .map(|(k, inner)| {
                    (
                        k.clone(),
                        inner
                            .iter()
                            .map(|(ik, t)| (ik.clone(), encode_portable_type(*t, table, interner)))
                            .collect(),
                    )
                })
                .collect(),
            type_members: encode_type_members(&bind.type_members, table, interner),
            class_parents: bind.class_parents.clone(),
            source_file: bind.source_file.clone(),
            sum_type_variants: bind.sum_type_variants.clone(),
            sum_variant_parent: bind.sum_variant_parent.clone(),
            sum_variant_fields: bind
                .sum_variant_fields
                .iter()
                .map(|(k, fields)| {
                    (
                        k.clone(),
                        fields
                            .iter()
                            .map(|(fname, t)| {
                                (fname.clone(), encode_portable_type(*t, table, interner))
                            })
                            .collect(),
                    )
                })
                .collect(),
            extensions: bind.extensions.clone(),
        }
    }

    /// Re-interna cada texto y cada tipo contra esta sesión, decodificando
    /// DIRECTAMENTE en `table` (la tabla viva del resolver). Una tabla nueva
    /// aquí dejaba los `CheckerTyId` de los símbolos decodificados válidos solo
    /// en esa tabla, y cualquier consumidor (p.ej. el prelude `core_exports`)
    /// los leía contra la suya: una forma distinta en el mismo índice. Decodificar
    /// en la compartida hace que todos vean los mismos ids (ADR-0011, Ley 2).
    fn into_live(
        self,
        interner: &mut AtomInterner,
        mut table_arc: std::sync::Arc<CheckerTyTable>,
    ) -> (ExportMap, BindResult) {
        let table = std::sync::Arc::make_mut(&mut table_arc);
        let exports = self
            .exports
            .into_iter()
            .map(|(k, v)| (k, decode_symbol(v, table, interner)))
            .collect();
        let mut arena = SymbolArena::default();
        for c in self.arena {
            arena.push(decode_symbol(c, table, interner));
        }
        let mut scopes = self.scopes;
        rebuild_scope_bindings(&mut scopes, &arena);

        let mut class_methods = FxHashMap::default();
        for (k, inner) in &self.class_methods {
            let mut m = FxHashMap::default();
            for (ik, t) in inner {
                m.insert(ik.clone(), decode_portable_type(t, table, interner));
            }
            class_methods.insert(k.clone(), m);
        }
        let type_members = decode_type_members(&self.type_members, table, interner);
        let mut sum_variant_fields = FxHashMap::default();
        for (k, fields) in &self.sum_variant_fields {
            sum_variant_fields.insert(
                k.clone(),
                fields
                    .iter()
                    .map(|(fname, t)| (fname.clone(), decode_portable_type(t, table, interner)))
                    .collect(),
            );
        }

        let bind = BindResult {
            arena,
            scopes,
            global_scope: self.global_scope,
            diagnostics: varn_core::DiagnosticBag::default(),
            interner: interner.clone(),
            ty_table: table_arc.clone(),
            class_methods,
            type_members,
            class_parents: self.class_parents,
            source_file: self.source_file,
            sum_type_variants: self.sum_type_variants,
            sum_variant_parent: self.sum_variant_parent,
            sum_variant_fields,
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
    resolver: Option<&dyn ImportResolver>,
) -> Result<Vec<u8>, String> {
    let cached = PortableModule::from_live(exports, bind, interner, resolver);
    postcard::to_allocvec(&cached).map_err(|e| e.to_string())
}

pub fn deserialize_module_interface(
    bytes: &[u8],
    interner: &mut AtomInterner,
    table: std::sync::Arc<CheckerTyTable>,
) -> Result<(ExportMap, BindResult), String> {
    let cached: PortableModule = postcard::from_bytes(bytes).map_err(|e| e.to_string())?;
    let (mut exports, bind) = cached.into_live(interner, table);
    super::exports::assign_slots(&mut exports);
    Ok((exports, bind))
}

pub(super) fn get_cache_dir(resolver: &super::DiskResolver) -> PathBuf {
    resolver.types_cache_dir()
}

/// The canonical identity for a cache entry: a `std:`/`core:`/`runtime:`
/// specifier or an absolute path. One function, so the checker and the
/// compiler name the same module the same way.
fn cache_module_id(virtual_id: &str) -> varn_core::ModuleId {
    if virtual_id.starts_with("std:")
        || virtual_id.starts_with("core:")
        || virtual_id.starts_with("runtime:")
    {
        varn_core::ModuleId::stdlib(virtual_id)
    } else {
        varn_core::ModuleId::local_str(virtual_id)
    }
}

/// The carrier a module's text came from. Two carriers of the same text can
/// shape the bind differently (a std module reads as `Bundle` from the embedded
/// bundle and as `File` from the checkout tree, and the difference reached
/// `origin_module`), so the cache key must include it: the invariant is
/// "(identity, bytes, carrier) ⇒ the same interface", and only then is serving
/// one where the other was requested safe.
pub(super) use super::CarrierKind;

fn cache_fingerprint(source: &str, carrier: CarrierKind) -> u64 {
    varn_modules::artifact::source_fingerprint(source) ^ ((carrier as u64) << 56)
}

pub(super) fn try_load_cache(
    resolver: &super::DiskResolver,
    virtual_id: &str,
    source: &str,
    carrier: CarrierKind,
) -> Option<CachedModule> {
    if virtual_id == "core:types" {
        return None;
    }
    let id = cache_module_id(virtual_id);
    let fingerprint = cache_fingerprint(source, carrier);
    let payload = varn_modules::artifact::read_module_artifact(
        &get_cache_dir(resolver),
        varn_modules::artifact::ArtifactKind::CheckerInterface,
        &id,
        fingerprint,
    )?;
    // Decode into the LIVE table so the ids every consumer reads are valid
    // (see `into_live`'s doc); publish the grown interner and table back.
    let table = resolver.ty_table_snapshot();
    let mut interner = resolver.interner_snapshot();
    let result = deserialize_module_interface(&payload, &mut interner, table);
    resolver.set_interner(interner);
    match result {
        Ok((exports, bind)) => {
            // Identity guard: even with the carrier in the key, a payload must
            // describe the module it was asked for. A mismatch is a miss,
            // never a wrong hit.
            let expected = varn_modules::canonical_or_original(std::path::Path::new(virtual_id));
            let got = bind.source_file.to_string();
            if got != virtual_id && got != expected {
                return None;
            }
            resolver.set_ty_table(bind.ty_table.clone());
            Some(CachedModule { exports, bind })
        }
        Err(_) => None,
    }
}

pub(super) fn save_to_cache(
    resolver: &super::DiskResolver,
    virtual_id: &str,
    source: &str,
    exports: &ExportMap,
    bind: &BindResult,
    carrier: CarrierKind,
) {
    if virtual_id == "core:types" {
        return;
    }
    let id = cache_module_id(virtual_id);
    let fingerprint = cache_fingerprint(source, carrier);
    let interner = resolver.interner_snapshot();
    if let Ok(payload) = serialize_module_interface(
        exports,
        bind,
        &interner,
        Some(&*resolver as &dyn ImportResolver),
    ) {
        varn_modules::artifact::write_module_artifact(
            &get_cache_dir(resolver),
            varn_modules::artifact::ArtifactKind::CheckerInterface,
            &id,
            fingerprint,
            &payload,
        );
    }
}
