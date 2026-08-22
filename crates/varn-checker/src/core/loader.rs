use crate::binder::BindResult;
use crate::module_resolver::{resolve_stdlib_module_bind_ref, resolve_stdlib_module_exports_ref};
use crate::symbol::Symbol;
use crate::types::{ClassMemberInfo, Type};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use varn_modules::spec::CORE_PREFIX;

thread_local! {
    static CORE_EXPORTS: RefCell<Option<Rc<FxHashMap<Rc<str>, Symbol>>>> = RefCell::new(None);
    static CORE_MEMBERS: RefCell<Option<Rc<CoreMembers>>> = RefCell::new(None);
}

#[derive(Clone, Default)]
pub struct CoreMembers {
    pub class_methods: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
    pub class_members: FxHashMap<Rc<str>, ClassMemberInfo>,
    pub interface_members: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub enum_members: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub namespace_members: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub flattened_members: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub class_parents: FxHashMap<Rc<str>, Rc<str>>,
    pub class_type_params: FxHashMap<Rc<str>, Vec<Rc<str>>>,
}

pub fn is_core_file(filename: &str) -> bool {
    filename.contains("varn-builtins") || filename.starts_with(CORE_PREFIX)
}

pub fn load_global_exports() -> FxHashMap<Rc<str>, Symbol> {
    CORE_EXPORTS.with(|c| {
        let mut guard = c.borrow_mut();
        if guard.is_none() {
            *guard = Some(Rc::new(build_core_exports()));
        }
        guard.as_ref().unwrap().as_ref().clone()
    })
}

pub fn global_exports_ref() -> Rc<FxHashMap<Rc<str>, Symbol>> {
    CORE_EXPORTS.with(|c| {
        let mut guard = c.borrow_mut();
        if guard.is_none() {
            *guard = Some(Rc::new(build_core_exports()));
        }
        Rc::clone(guard.as_ref().unwrap())
    })
}

pub fn core_members_ref() -> Rc<CoreMembers> {
    CORE_MEMBERS.with(|c| {
        let mut guard = c.borrow_mut();
        if guard.is_none() {
            *guard = Some(Rc::new(build_core_members()));
        }
        Rc::clone(guard.as_ref().unwrap())
    })
}

pub fn merge_core_members(bind: &mut BindResult) {
    bind.core = Some(core_members_ref());
}

fn build_core_exports() -> FxHashMap<Rc<str>, Symbol> {
    let mut globals = FxHashMap::default();
    for spec in varn_modules::core_module_ids() {
        for (k, v) in resolve_stdlib_module_exports_ref(spec).as_ref() {
            globals.insert(Rc::from(k.as_str()), v.clone());
        }
    }
    globals
}

fn build_core_members() -> CoreMembers {
    let mut members = CoreMembers::default();
    for spec in varn_modules::core_module_ids() {
        if let Some(rb) = resolve_stdlib_module_bind_ref(spec) {
            let scope = rb.scopes.get(rb.global_scope);
            for (name, &sid) in &scope.bindings {
                let sym = rb.arena.get(sid);
                if !sym.type_params.is_empty() {
                    members
                        .class_type_params
                        .insert(name.clone(), sym.type_params.clone());
                }
            }

            for (k, v) in &rb.type_members.classes {
                let mut v = v.clone();
                v.is_builtin_or_intrinsic = true;
                members.class_members.insert(k.clone(), v);
            }
            for (k, v) in &rb.type_members.interfaces {
                members.interface_members.insert(k.clone(), v.clone());
            }
            for (k, v) in &rb.type_members.enums {
                members.enum_members.insert(k.clone(), v.clone());
            }
            for (k, v) in &rb.type_members.namespaces {
                members.namespace_members.insert(k.clone(), v.clone());
            }
            for (k, v) in &rb.class_methods {
                members.class_methods.insert(k.clone(), v.clone());
            }
            for (k, v) in &rb.type_members.flattened {
                members.flattened_members.insert(k.clone(), v.clone());
            }
            for (k, v) in &rb.class_parents {
                members.class_parents.insert(k.clone(), v.clone());
            }
        }
    }
    members
}
