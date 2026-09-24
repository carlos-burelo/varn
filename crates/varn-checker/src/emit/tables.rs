//! The module tables: classes with their layout and vtable, enums, and the
//! signatures both reference. Built before any body, because `Class(ClassId)`
//! and `Enum(EnumId)` need the handle.
//!
//! Sub-phase 1b: real classes, enums, vtables and method signatures from
//! `bind`. Free-function signatures and native-class prefixes come later.

use crate::binder::BindResult;
use crate::emit::ty::{lower_type, NameResolver};
use crate::types::{CheckerTyTable, ClassMemberKind, Type};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::{AtomInterner, TypeKind};
use varn_tir::{BackendTy, ClassId, ClassInfo, EnumId, EnumInfo, Signature, TyTable, VariantInfo};

/// Name → module-table handle, for the class and enum types the checker
/// proved local to this module. An imported or unknown name answers `None`
/// and lowers to `Dynamic(NotYetSupported)`.
#[derive(Default)]
pub struct NameIndex {
    module: Arc<str>,
    classes: FxHashMap<Arc<str>, ClassId>,
    enums: FxHashMap<Arc<str>, EnumId>,
    foreign_enums: FxHashMap<(Arc<str>, Arc<str>), EnumId>,
}

impl NameResolver for NameIndex {
    fn class_id(&self, name: &str) -> Option<ClassId> {
        self.classes.get(name).copied()
    }
    fn enum_id(&self, name: &str) -> Option<EnumId> {
        self.enums.get(name).copied()
    }
    fn foreign_enum_id(&self, name: &str, origin: &str) -> Option<EnumId> {
        self.foreign_enums
            .get(&(Arc::from(name), Arc::from(origin)))
            .copied()
    }
    fn is_local_origin(&self, origin: &str) -> bool {
        origin == self.module.as_ref()
    }
}

pub struct Tables {
    pub classes: Vec<ClassInfo>,
    pub enums: Vec<EnumInfo>,
    pub signatures: Vec<Signature>,
    /// Consumed by the body emitter (sub-phase 4) to resolve `Named` types and
    /// `New`; unused while bodies are still stubs.
    #[allow(dead_code)]
    pub names: NameIndex,
}

/// Signature 0 is always the empty `() -> void`, so a function with no
/// computed signature still names a real entry.
fn seed_signatures() -> Vec<Signature> {
    vec![Signature {
        params: vec![],
        return_ty: BackendTy::Void,
    }]
}

pub fn build(
    bind: &BindResult,
    foreign: &[crate::checker::ForeignEnum],
    tt: &mut TyTable,
) -> Tables {
    // Pass 1: assign every local class and enum a stable handle. `classes` and
    // `enums` in `Tables` are filled in this same index order.
    let mut class_names: Vec<Arc<str>> = bind.type_members.classes.keys().cloned().collect();
    class_names.sort();
    // A name that is really an enum is registered under `enums`, not `classes`
    // — the binder puts enum decls in `type_members.classes` too (as the
    // value type) and separately in `type_members.enums` (the variants).
    class_names.retain(|n| !bind.type_members.enums.contains_key(n));

    // Plain enums live in `type_members.enums`; payload ("sum type") enums
    // live in `sum_type_variants` and their fields in `sum_variant_fields`.
    let mut enum_names: Vec<Arc<str>> = bind
        .type_members
        .enums
        .keys()
        .chain(bind.sum_type_variants.keys())
        .cloned()
        .collect();
    enum_names.sort();
    enum_names.dedup();
    class_names.retain(|n| !bind.sum_type_variants.contains_key(n));

    let mut names = NameIndex {
        module: Arc::from(bind.source_file.as_ref()),
        ..NameIndex::default()
    };
    for (i, f) in foreign.iter().enumerate() {
        names.foreign_enums.insert(
            (f.name.clone(), f.origin.clone()),
            EnumId((enum_names.len() + i) as u32),
        );
    }
    for (i, n) in class_names.iter().enumerate() {
        names.classes.insert(n.clone(), ClassId(i as u32));
    }
    for (i, n) in enum_names.iter().enumerate() {
        names.enums.insert(n.clone(), EnumId(i as u32));
    }

    // Pass 2: build the infos. Classes in parent-before-child order so
    // `ClassInfo::new_with_methods` can take the parent's `&ClassInfo`.
    let mut signatures = seed_signatures();
    let classes = build_classes(
        bind,
        &bind.ty_table,
        &bind.interner,
        tt,
        &names,
        &class_names,
        &mut signatures,
    );
    let mut enums = build_enums(
        bind,
        &bind.ty_table,
        &bind.interner,
        tt,
        &names,
        &enum_names,
    );
    // A foreign enum's payload types live in its own module's table, which
    // this one cannot read: the fields are erased to `Dynamic`, the tags come
    // from the declaring module's layout.
    enums.extend(foreign.iter().map(|f| EnumInfo {
        name: f.name.clone(),
        variants: f
            .variants
            .iter()
            .enumerate()
            .map(|(tag, (name, fields))| VariantInfo {
                name: name.clone(),
                tag: tag as u16,
                payload: vec![BackendTy::Dynamic(varn_tir::DynReason::Unannotated); *fields],
            })
            .collect(),
    }));

    Tables {
        classes,
        enums,
        signatures,
        names,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_classes(
    bind: &BindResult,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    class_names: &[Arc<str>],
    signatures: &mut Vec<Signature>,
) -> Vec<ClassInfo> {
    // A class is laid out after its parent: its fields and vtable extend the
    // parent's, with the slots the runtime's inherited shape uses. Ids follow
    // `class_names` (sorted); the build follows the `extends` chain.
    let mut built: Vec<Option<ClassInfo>> = vec![None; class_names.len()];
    for i in 0..class_names.len() {
        build_with_parents(bind, table, interner, tt, names, class_names, i, signatures, &mut built);
    }
    built.into_iter().map(|c| c.expect("every class is built")).collect()
}

/// Builds class `i` after its local ancestors. A cycle in `extends` (reported
/// by the checker) leaves the class that closes it without parent info.
#[allow(clippy::too_many_arguments)]
fn build_with_parents(
    bind: &BindResult,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    class_names: &[Arc<str>],
    i: usize,
    signatures: &mut Vec<Signature>,
    built: &mut [Option<ClassInfo>],
) {
    let mut chain = vec![i];
    let mut cur = i;
    while let Some(p) = bind
        .class_parents
        .get(&class_names[cur])
        .and_then(|p| names.class_id(p))
        .map(|id| id.0 as usize)
    {
        if built[p].is_some() || chain.contains(&p) {
            break;
        }
        chain.push(p);
        cur = p;
    }
    for &c in chain.iter().rev() {
        if built[c].is_none() {
            let info =
                build_one_class(bind, table, interner, tt, names, &class_names[c], signatures, built);
            built[c] = Some(info);
        }
    }
}

/// The binder's `type_members.classes[name].members` is already flattened —
/// parent members first, in slot order, with an `override` appearing under
/// both the base and the derived name. So each class is built standalone
/// (`parent: None`), deduping fields by first sighting (the declared slot) and
/// methods by last (the most-derived signature).
#[allow(clippy::too_many_arguments)]
fn build_one_class(
    bind: &BindResult,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    name: &Arc<str>,
    signatures: &mut Vec<Signature>,
    built: &[Option<ClassInfo>],
) -> ClassInfo {
    let parent_name = bind.class_parents.get(name);
    let parent_id = parent_name.and_then(|p| names.class_id(p));
    let parent_info = parent_id.and_then(|id| built.get(id.0 as usize)?.as_ref());
    let mut inherited_fields: FxHashMap<Arc<str>, ()> = parent_info
        .map(|p| p.fields.iter().map(|f| (f.name.clone(), ())).collect())
        .unwrap_or_default();

    // A class extending a NATIVE class the local table doesn't hold: the only
    // user-extensible one is the `Error` family, whose instances carry
    // `message` / `name` / `stack` before any own field. The runtime's
    // `op_inherit` lays them out first, so the own fields' slots must too.
    let mut fields: Vec<(Arc<str>, BackendTy)> = Vec::new();
    if parent_name.is_some() && parent_id.is_none() {
        for f in ["message", "name", "stack"] {
            fields.push((Arc::from(f), BackendTy::Str));
            inherited_fields.insert(Arc::from(f), ());
        }
    }
    let members = bind
        .type_members
        .classes
        .get(name)
        .map(|e| e.members.as_slice())
        .unwrap_or(&[]);

    let mut seen_field: FxHashMap<Arc<str>, ()> = FxHashMap::default();
    let mut method_names: Vec<Arc<str>> = Vec::new();
    let mut method_sig: FxHashMap<Arc<str>, varn_tir::SigId> = FxHashMap::default();

    let mut push_method = |key: Arc<str>, sig: varn_tir::SigId, order: &mut Vec<Arc<str>>| {
        if method_sig.insert(key.clone(), sig).is_none() {
            order.push(key);
        }
    };

    for m in members {
        if m.is_static {
            continue;
        }
        match m.kind {
            ClassMemberKind::Property | ClassMemberKind::Variable => {
                // Own fields only — inherited ones are laid out by the parent
                // prefix `new_with_methods` prepends.
                if !inherited_fields.contains_key(&m.name)
                    && seen_field.insert(m.name.clone(), ()).is_none()
                {
                    // `is_optional` on a class field (set by
                    // `binder::class::bind_class` when no constructor
                    // guarantees it) means the read can yield `null` at
                    // runtime. `member_type.rs` already exposes it as
                    // `T | null` to the checker — this is the OTHER path to
                    // the same field (`field_access` via `ClassInfo.fields`),
                    // so wrap at the BackendTy level too. Done here (rather
                    // than interning a new `CheckerTy` union) because this
                    // function only holds `&CheckerTyTable`.
                    let inner = lower_type(&m.ty, table, interner, tt, names);
                    let field_bt = if m.is_optional && !matches!(inner, BackendTy::Nullable(_)) {
                        BackendTy::Nullable(tt.intern(inner))
                    } else {
                        inner
                    };
                    fields.push((m.name.clone(), field_bt));
                }
            }
            ClassMemberKind::Method | ClassMemberKind::Function => {
                let sig = intern_signature(&m.ty, table, interner, tt, names, signatures);
                push_method(m.name.clone(), sig, &mut method_names);
            }
            ClassMemberKind::Getter => {
                let sig = intern_signature(&m.ty, table, interner, tt, names, signatures);
                push_method(Arc::from(format!("get {}", m.name)), sig, &mut method_names);
            }
            ClassMemberKind::Setter => {
                let sig = intern_signature(&m.ty, table, interner, tt, names, signatures);
                push_method(Arc::from(format!("set {}", m.name)), sig, &mut method_names);
            }
            _ => {}
        }
    }

    let methods: Vec<(Arc<str>, varn_tir::SigId)> = method_names
        .into_iter()
        .map(|n| (n.clone(), method_sig[&n]))
        .collect();

    let parent_arg = parent_id.zip(parent_info).map(|(id, info)| (id, info));
    ClassInfo::new_with_methods(name.clone(), parent_arg, fields, methods)
}

/// Append a signature for a method and hand back its id.
///
/// Sub-phase 3 keeps only the arity: params and return are
/// `Dynamic(NotYetSupported)`. Precise method-signature typing is its own
/// sub-phase — an un-annotated method return reads as `Void` off the binder
/// today, which would make every `return x` inside it fail coherence.
fn intern_signature(
    ty: &Type,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    signatures: &mut Vec<Signature>,
) -> varn_tir::SigId {
    let (params, return_ty) = match table.get(ty.0) {
        TypeKind::Fn(fid) => {
            let ft = table.get_function(fid);
            let p_tys: Vec<BackendTy> = ft
                .params
                .iter()
                .map(|p| lower_type(&Type(p.ty, false), table, interner, tt, names))
                .collect();
            (p_tys, BackendTy::Dynamic(varn_tir::DynReason::Unannotated))
        }
        _ => (vec![], BackendTy::Dynamic(varn_tir::DynReason::Unannotated)),
    };
    let id = signatures.len() as u32;
    signatures.push(Signature { params, return_ty });
    varn_tir::SigId(id)
}

fn build_enums(
    bind: &BindResult,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    enum_names: &[Arc<str>],
) -> Vec<EnumInfo> {
    enum_names
        .iter()
        .map(|name| EnumInfo {
            name: name.clone(),
            variants: bind
                .enum_layout(name)
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .map(|(tag, (variant, fields))| VariantInfo {
                    name: variant,
                    tag: tag as u16,
                    payload: fields
                        .iter()
                        .map(|(_, ty)| lower_type(ty, table, interner, tt, names))
                        .collect(),
                })
                .collect(),
        })
        .collect()
}
