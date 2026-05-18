use crate::binder::BindResult;
use crate::module_resolver::{resolve_stdlib_module_bind, resolve_stdlib_module_exports};
use crate::symbol::Symbol;
use crate::types::{ClassMemberInfo, Type};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use varn_modules::spec::BUILTIN_PREFIX;

thread_local! {
    static BUILTIN_EXPORTS: RefCell<Option<Rc<FxHashMap<Rc<str>, Symbol>>>> = RefCell::new(None);
    static BUILTIN_MEMBERS: RefCell<Option<Rc<BuiltinMembers>>> = RefCell::new(None);
}

#[derive(Clone, Default)]
pub struct BuiltinMembers {
    pub class_methods: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
    pub class_members: FxHashMap<Rc<str>, ClassMemberInfo>,
    pub interface_members: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub enum_members: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub namespace_members: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub flattened_members: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub class_parents: FxHashMap<Rc<str>, Rc<str>>,
    pub class_type_params: FxHashMap<Rc<str>, Vec<Rc<str>>>,
}

pub fn is_builtin_file(filename: &str) -> bool {
    filename.contains("varn-stdlib/builtins")
        || filename.contains(r"varn-stdlib\builtins")
        || filename.contains("varn-builtins")
        || filename.starts_with(BUILTIN_PREFIX)
}

pub fn load_global_exports() -> FxHashMap<Rc<str>, Symbol> {
    BUILTIN_EXPORTS.with(|c| {
        let mut guard = c.borrow_mut();
        if guard.is_none() {
            *guard = Some(Rc::new(build_builtin_exports()));
        }
        guard.as_ref().unwrap().as_ref().clone()
    })
}

pub fn global_exports_ref() -> Rc<FxHashMap<Rc<str>, Symbol>> {
    BUILTIN_EXPORTS.with(|c| {
        let mut guard = c.borrow_mut();
        if guard.is_none() {
            *guard = Some(Rc::new(build_builtin_exports()));
        }
        Rc::clone(guard.as_ref().unwrap())
    })
}

pub fn builtin_members_ref() -> Rc<BuiltinMembers> {
    BUILTIN_MEMBERS.with(|c| {
        let mut guard = c.borrow_mut();
        if guard.is_none() {
            *guard = Some(Rc::new(build_builtin_members()));
        }
        Rc::clone(guard.as_ref().unwrap())
    })
}

pub fn merge_builtin_members(bind: &mut BindResult) {
    bind.builtin = Some(builtin_members_ref());
}

fn build_builtin_exports() -> FxHashMap<Rc<str>, Symbol> {
    let mut globals = FxHashMap::default();
    for spec in varn_modules::BUILTIN_MODULES {
        for (k, v) in resolve_stdlib_module_exports(spec) {
            globals.insert(Rc::from(k.as_str()), v);
        }
    }
    globals
}

fn build_builtin_members() -> BuiltinMembers {
    let mut members = BuiltinMembers::default();
    for spec in varn_modules::BUILTIN_MODULES {
        if let Some(rb) = resolve_stdlib_module_bind(spec) {
            let scope = rb.scopes.get(rb.global_scope);
            for (name, &sid) in &scope.bindings {
                let sym = rb.arena.get(sid);
                if !sym.type_params.is_empty() {
                    members
                        .class_type_params
                        .insert(name.clone(), sym.type_params.clone());
                }
            }

            for (k, v) in rb.type_members.classes {
                members.class_members.insert(k, v);
            }
            members.interface_members.extend(rb.type_members.interfaces);
            members.enum_members.extend(rb.type_members.enums);
            members.namespace_members.extend(rb.type_members.namespaces);
            members.class_methods.extend(rb.class_methods);
            members.flattened_members.extend(rb.type_members.flattened);
            members.class_parents.extend(rb.class_parents);
        }
    }
    members
}
