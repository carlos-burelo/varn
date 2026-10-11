use std::sync::Arc;
use varn_sem::bind::BindResult;
use varn_sem::exports::ExportMap;
use varn_sem::symbol::{Symbol, SymbolKind};
use varn_sem::types::{ClassMemberInfo, ClassMemberKind};

pub(super) fn export_declared(
    bind: &BindResult,
    declared: &str,
    exported: &str,
    abs_path: &str,
    out: &mut ExportMap,
) {
    let Some(listed) = bind.get_namespace_members_local(declared) else {
        return;
    };
    let mut paths = Vec::new();
    value_paths(listed, "", &mut paths);
    for (path, member) in paths {
        let table = Arc::make_mut(&mut out.table);
        let key = format!("{exported}::{path}");
        let mut sym = Symbol::new(SymbolKind::Const, table.intern_name(&key), member.line);
        sym.ty = Some(member.ty);
        sym.origin_module = Some(table.intern_name(abs_path));
        sym.original_name = Some(table.intern_name(&key));
        out.insert(key, sym);
    }
}

pub(super) fn forward(src: &ExportMap, local: &str, exported: &str, out: &mut ExportMap) {
    let prefix = format!("{local}::");
    let mut keys: Vec<&String> = src.keys().filter(|k| k.starts_with(&prefix)).collect();
    keys.sort();
    for key in keys {
        let Some(sym) = src.get(key) else {
            continue;
        };
        out.insert(format!("{exported}::{}", &key[prefix.len()..]), sym.clone());
    }
}

pub(super) fn carry_imported(
    bind: &BindResult,
    binding: &Symbol,
    root: &str,
    members: &[Arc<str>],
    exported: &str,
    out: &mut ExportMap,
) {
    let Some(origin) = binding
        .origin_module
        .and_then(|a| bind.interner.try_resolve(a).map(str::to_owned))
    else {
        return;
    };
    for member in members {
        let table = Arc::make_mut(&mut out.table);
        let key = format!("{exported}::{member}");
        let mut sym = Symbol::new(SymbolKind::Const, table.intern_name(&key), 0);
        sym.origin_module = Some(table.intern_name(&origin));
        sym.original_name = Some(table.intern_name(&format!("{root}::{member}")));
        out.insert(key, sym);
    }
}

fn value_paths<'a>(
    listed: &'a [ClassMemberInfo],
    outer: &str,
    out: &mut Vec<(String, &'a ClassMemberInfo)>,
) {
    for member in listed {
        let path = if outer.is_empty() {
            member.name.to_string()
        } else {
            format!("{outer}::{}", member.name)
        };
        match member.kind {
            ClassMemberKind::Function
            | ClassMemberKind::Variable
            | ClassMemberKind::Class
            | ClassMemberKind::Enum => out.push((path, member)),
            ClassMemberKind::Namespace => value_paths(&member.members, &path, out),
            ClassMemberKind::Constructor
            | ClassMemberKind::Method
            | ClassMemberKind::Property
            | ClassMemberKind::Getter
            | ClassMemberKind::Setter
            | ClassMemberKind::Interface
            | ClassMemberKind::Struct => {}
        }
    }
}
