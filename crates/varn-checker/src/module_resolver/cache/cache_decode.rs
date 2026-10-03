use super::cache_types::{
    PortableClassMemberInfo, PortableModule, PortableSymbol, PortableTypeMembers,
};
use crate::binder::{BindResult, TypeMembers};
use crate::scope::ScopeArena;
use crate::symbol::{Symbol, SymbolArena};
use crate::types::{decode_portable_type, encode_portable_type, CheckerTyTable, ClassMemberInfo};
use rustc_hash::FxHashMap;
use varn_core::AtomInterner;

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

pub(super) fn decode_member(
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

pub(super) fn decode_member_list(
    list: &[PortableClassMemberInfo],
    table: &mut CheckerTyTable,
    interner: &mut AtomInterner,
) -> Vec<ClassMemberInfo> {
    list.iter()
        .map(|m| decode_member(m, table, interner))
        .collect()
}

pub(super) fn decode_type_members(
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

impl PortableModule {
    pub(super) fn from_live(
        exports: &super::super::ExportMap,
        bind: &BindResult,
        interner: &AtomInterner,
        resolver: Option<&dyn super::super::resolver_api::ImportResolver>,
    ) -> Self {
        let table = &bind.ty_table;
        Self {
            exports: exports
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        super::cache_encode::encode_symbol(v, table, interner, resolver),
                    )
                })
                .collect(),
            arena: bind
                .arena
                .all()
                .iter()
                .map(|s| super::cache_encode::encode_symbol(s, table, interner, resolver))
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
            type_members: super::cache_encode::encode_type_members(
                &bind.type_members,
                table,
                interner,
            ),
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

    pub(super) fn into_live(
        self,
        interner: &mut AtomInterner,
        mut table_arc: std::sync::Arc<CheckerTyTable>,
    ) -> (super::super::ExportMap, BindResult) {
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

pub(super) fn rebuild_scope_bindings(scopes: &mut ScopeArena, arena: &SymbolArena) {
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
