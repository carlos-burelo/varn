//! The module tables: classes with their layout and vtable, enums, and the
//! signatures both reference. Built before any body, because `Class(ClassId)`
//! and `Enum(EnumId)` need the handle.
//!
//! Sub-phase 1b: real classes, enums, vtables and method signatures from
//! `bind`. Free-function signatures and native-class prefixes come later.

use crate::binder::BindResult;
use crate::emit::ty::{lower_type, NameResolver};
use crate::types::{ClassMemberKind, FunctionType, Type};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::TypeKind;
use varn_tir::{BackendTy, ClassId, ClassInfo, EnumId, EnumInfo, Signature, TyTable, VariantInfo};

/// Name → module-table handle, for the class and enum types the checker
/// proved local to this module. An imported or unknown name answers `None`
/// and lowers to `Dynamic(NotYetSupported)`.
#[derive(Default)]
pub struct NameIndex {
    classes: FxHashMap<Rc<str>, ClassId>,
    enums: FxHashMap<Rc<str>, EnumId>,
}

impl NameResolver for NameIndex {
    fn class_id(&self, name: &str) -> Option<ClassId> {
        self.classes.get(name).copied()
    }
    fn enum_id(&self, name: &str) -> Option<EnumId> {
        self.enums.get(name).copied()
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
    vec![Signature { params: vec![], return_ty: BackendTy::Void }]
}

pub fn build(bind: &BindResult, tt: &mut TyTable) -> Tables {
    // Pass 1: assign every local class and enum a stable handle. `classes` and
    // `enums` in `Tables` are filled in this same index order.
    let mut class_names: Vec<Rc<str>> = bind.type_members.classes.keys().cloned().collect();
    class_names.sort();
    // A name that is really an enum is registered under `enums`, not `classes`
    // — the binder puts enum decls in `type_members.classes` too (as the
    // value type) and separately in `type_members.enums` (the variants).
    class_names.retain(|n| !bind.type_members.enums.contains_key(n));

    let mut enum_names: Vec<Rc<str>> = bind.type_members.enums.keys().cloned().collect();
    enum_names.sort();

    let mut names = NameIndex::default();
    for (i, n) in class_names.iter().enumerate() {
        names.classes.insert(n.clone(), ClassId(i as u32));
    }
    for (i, n) in enum_names.iter().enumerate() {
        names.enums.insert(n.clone(), EnumId(i as u32));
    }

    // Pass 2: build the infos. Classes in parent-before-child order so
    // `ClassInfo::new_with_methods` can take the parent's `&ClassInfo`.
    let mut signatures = seed_signatures();
    let classes = build_classes(bind, tt, &names, &class_names, &mut signatures);
    let enums = build_enums(bind, tt, &names, &enum_names);

    Tables { classes, enums, signatures, names }
}

fn build_classes(
    bind: &BindResult,
    tt: &mut TyTable,
    names: &NameIndex,
    class_names: &[Rc<str>],
    signatures: &mut Vec<Signature>,
) -> Vec<ClassInfo> {
    class_names
        .iter()
        .map(|name| build_one_class(bind, tt, names, name, signatures))
        .collect()
}

/// The binder's `type_members.classes[name].members` is already flattened —
/// parent members first, in slot order, with an `override` appearing under
/// both the base and the derived name. So each class is built standalone
/// (`parent: None`), deduping fields by first sighting (the declared slot) and
/// methods by last (the most-derived signature).
fn build_one_class(
    bind: &BindResult,
    tt: &mut TyTable,
    names: &NameIndex,
    name: &Rc<str>,
    signatures: &mut Vec<Signature>,
) -> ClassInfo {
    let members = bind
        .type_members
        .classes
        .get(name)
        .map(|e| e.members.as_slice())
        .unwrap_or(&[]);

    let mut fields: Vec<(Rc<str>, BackendTy)> = Vec::new();
    let mut seen_field: FxHashMap<Rc<str>, ()> = FxHashMap::default();
    let mut method_names: Vec<Rc<str>> = Vec::new();
    let mut method_sig: FxHashMap<Rc<str>, varn_tir::SigId> = FxHashMap::default();

    let mut push_method =
        |key: Rc<str>, sig: varn_tir::SigId, order: &mut Vec<Rc<str>>| {
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
                if seen_field.insert(m.name.clone(), ()).is_none() {
                    fields.push((m.name.clone(), lower_type(&m.ty, tt, names)));
                }
            }
            ClassMemberKind::Method | ClassMemberKind::Function => {
                let sig = intern_signature(&m.ty, tt, names, signatures);
                push_method(m.name.clone(), sig, &mut method_names);
            }
            ClassMemberKind::Getter => {
                let sig = intern_signature(&m.ty, tt, names, signatures);
                push_method(Rc::from(format!("get {}", m.name)), sig, &mut method_names);
            }
            ClassMemberKind::Setter => {
                let sig = intern_signature(&m.ty, tt, names, signatures);
                push_method(Rc::from(format!("set {}", m.name)), sig, &mut method_names);
            }
            _ => {}
        }
    }

    let methods: Vec<(Rc<str>, varn_tir::SigId)> =
        method_names.into_iter().map(|n| (n.clone(), method_sig[&n])).collect();

    let mut info = ClassInfo::new_with_methods(name.clone(), None, fields, methods);
    info.parent = bind.class_parents.get(name).and_then(|p| names.class_id(p));
    info
}

/// Lower a function type to a `Signature`, append it, and hand back its id. A
/// member whose type is not `Fn` (a malformed method) still gets the empty
/// signature 0, which verifies.
fn intern_signature(
    ty: &Type,
    tt: &mut TyTable,
    names: &NameIndex,
    signatures: &mut Vec<Signature>,
) -> varn_tir::SigId {
    let TypeKind::Fn(FunctionType { params, return_type, .. }) = ty.kind() else {
        return varn_tir::SigId(0);
    };
    let params: Vec<BackendTy> = params.iter().map(|p| lower_type(&p.ty, tt, names)).collect();
    let return_ty = lower_type(return_type, tt, names);
    let id = signatures.len() as u32;
    signatures.push(Signature { params, return_ty });
    varn_tir::SigId(id)
}

fn build_enums(
    bind: &BindResult,
    tt: &mut TyTable,
    names: &NameIndex,
    enum_names: &[Rc<str>],
) -> Vec<EnumInfo> {
    enum_names
        .iter()
        .map(|name| {
            let variants = bind
                .type_members
                .enums
                .get(name)
                .map(|vs| vs.as_slice())
                .unwrap_or(&[]);
            let variants = variants
                .iter()
                .enumerate()
                .map(|(tag, v)| VariantInfo {
                    name: v.name.clone(),
                    tag: tag as u16,
                    payload: v
                        .members
                        .iter()
                        .map(|f| lower_type(&f.ty, tt, names))
                        .collect(),
                })
                .collect();
            EnumInfo { name: name.clone(), variants }
        })
        .collect()
}
