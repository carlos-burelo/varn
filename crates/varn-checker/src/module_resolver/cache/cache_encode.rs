use super::cache_types::{PortableClassMemberInfo, PortableSymbol, PortableTypeMembers};
use crate::binder::TypeMembers;
use crate::module_resolver::ImportResolver;
use crate::symbol::Symbol;
use crate::types::{
    encode_portable_type, CheckerTyTable, ClassMemberInfo, Type,
};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::{Atom, AtomInterner};

pub(super) fn encode_with_owner(
    ty: Type,
    origin: Option<Atom>,
    fallback_table: &CheckerTyTable,
    resolver: Option<&dyn ImportResolver>,
    interner: &AtomInterner,
) -> crate::types::PortableType {
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
    crate::types::PortableType::Primitive(varn_core::LangPrimitive::Dynamic)
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

pub(super) fn encode_member(
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

pub(super) fn encode_member_list(
    list: &[ClassMemberInfo],
    table: &CheckerTyTable,
    interner: &AtomInterner,
) -> Vec<PortableClassMemberInfo> {
    list.iter()
        .map(|m| encode_member(m, table, interner))
        .collect()
}

pub(super) fn encode_type_members(
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
