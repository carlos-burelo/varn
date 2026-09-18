use crate::core::loader::CoreMembers;
use crate::scope::{ScopeArena, ScopeId};
use crate::symbol::{Symbol, SymbolArena, SymbolId};
use crate::types::{ClassMemberInfo, Type};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::{ExprId, StmtId, TypeNode};
use varn_core::Atom;

pub use crate::types::TypeContext;

// Node identity used to be a raw pointer into a heap-allocated `Expr`/`Stmt`
// (fragile: the pointee's address was only valid while that `Box` lived).
// Now that expressions/statements live in an `AstArena` addressed by id
// (fase1-componente2), the natural — and inherently `Send`/`Sync` — handle
// is the id itself; no `unsafe impl` needed anymore.
#[derive(Clone)]
pub enum PendingEnrich {
    Var {
        sym_id: SymbolId,
        init: ExprId,
    },
    Fn {
        sym_id: SymbolId,
        body: StmtId,
        is_async: bool,
    },
    Method {
        // `class_name`/`key` forward the class/member name `Atom`s the
        // binder already resolved from the AST — no synthetic text here,
        // so this stays a handle instead of re-wrapping into `Rc<str>`.
        class_name: Atom,
        key: Atom,
        body: StmtId,
        is_async: bool,
    },
    Getter {
        class_name: Atom,
        key: Atom,
        body: StmtId,
    },
    Setter {
        class_name: Atom,
        key: Atom,
        body: StmtId,
    },
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TypeMembers {
    pub classes: FxHashMap<Rc<str>, ClassMemberInfo>,
    pub interfaces: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    // Object-literal declarator names are looked up by `Atom` at every call
    // site that still compiles (`Decl::Struct.id`, `Pattern::Identifier.name`),
    // unlike the other maps here which are queried by `&str` through
    // `TypeContext`/`BindResult` accessors — so this one forwards the AST's
    // `Atom` directly instead of re-wrapping into `Rc<str>`. `Atom` does not
    // (and should not) derive `serde::{Serialize, Deserialize}` — a bare
    // interned index is meaningless without the matching `AtomInterner`, so
    // this field, like the binder's other in-process-only data, is skipped.
    #[serde(skip)]
    pub objects: FxHashMap<Atom, Vec<ClassMemberInfo>>,
    pub enums: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub namespaces: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub flattened: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub getters: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
    pub setters: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Extensions {
    pub methods: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Rc<str>>>,
    pub getters: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Rc<str>>>,
    pub setters: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Rc<str>>>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct BindResult {
    pub arena: SymbolArena,
    pub scopes: ScopeArena,
    pub global_scope: ScopeId,
    #[serde(skip)]
    pub diagnostics: varn_core::DiagnosticBag,
    /// Resolves the `Atom`s carried by this bind result (`PendingEnrich`,
    /// `type_members.objects`, ...) back to text — diagnostics and any
    /// comparison against externally-supplied text need it.
    ///
    /// NOT YET WIRED to the real per-parse interner `varn_parser::parse`
    /// returns: threading it here would require changing the signature of
    /// `Binder::bind`/`bind_with_global_refs`, whose callers
    /// (`crate::checker::mod`, `crate::module_resolver::resolver`,
    /// `varn-cli/src/debug_binder.rs`) are outside this task's 3-file scope
    /// and are already broken pending their own migration tasks. Until a
    /// later task threads the real interner through, this is a placeholder
    /// `AtomInterner::new()` built in `Binder::bind_with_globals_iter` —
    /// resolving an `Atom` interned by the *real* parser interner against
    /// this placeholder will panic (empty table). See task-4-report.md.
    #[serde(skip)]
    pub interner: varn_core::AtomInterner,
    pub class_methods: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
    pub type_members: TypeMembers,
    pub class_parents: FxHashMap<Rc<str>, Rc<str>>,
    pub source_file: Rc<str>,
    pub sum_type_variants: FxHashMap<Rc<str>, Vec<Rc<str>>>,
    pub sum_variant_parent: FxHashMap<Rc<str>, Rc<str>>,
    pub sum_variant_fields: FxHashMap<Rc<str>, Vec<(Rc<str>, Type)>>,
    pub extensions: Extensions,
    #[serde(skip)]
    pub core: Option<Rc<CoreMembers>>,
    #[serde(skip)]
    pub pending_enrich: Vec<PendingEnrich>,
    /// Advisory element types for evolving empty-array locals (Task A0.3'),
    /// keyed by the declarator identifier's source offset → the proved
    /// `Array<T>`. This is an OPTIMIZATION-ONLY channel: it feeds codegen
    /// type annotations (`collect_type_annotations`) and NOTHING ELSE —
    /// never `symbol_types`, `resolved_expr_types`, `types_compatible`, or
    /// member-existence reads. Keeping it out of the diagnostic path is what
    /// guarantees design rule 4 ("zero new type errors"): narrowing an
    /// `x[i]` read from `Dynamic` to `int` can never make a previously-valid
    /// program fail `vn check`. Not serialized — it is consumed in-process
    /// immediately after binding, and function-local evolved arrays are
    /// never exported, so a reloaded (cached) `BindResult` needs no entries.
    #[serde(skip, default)]
    pub evolved_array_types: FxHashMap<u32, Type>,
}

impl BindResult {
    pub fn global_symbols(&self) -> impl Iterator<Item = &Symbol> {
        let scope = self.scopes.get(self.global_scope);
        scope.ordered.iter().map(|&id| self.arena.get(id))
    }

    pub fn get_class_entry(&self, name: &str) -> Option<&ClassMemberInfo> {
        self.type_members
            .classes
            .get(name)
            .or_else(|| self.core.as_ref().and_then(|b| b.class_members.get(name)))
    }

    /// Determines whether `name` identifies a user-defined class or struct with fixed field
    /// slot layout in the VM heap (excluding intrinsic / primitive built-in types).
    #[inline]
    pub fn is_user_class(&self, name: &str) -> bool {
        if varn_core::IntrinsicType::is_intrinsic(name) {
            return false;
        }
        self.get_class_entry(name)
            .map(|entry| !entry.is_builtin_or_intrinsic)
            .unwrap_or(false)
    }

    pub fn get_interface_members_local(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members.interfaces.get(name).or_else(|| {
            self.core
                .as_ref()
                .and_then(|b| b.interface_members.get(name))
        })
    }

    pub fn get_namespace_members_local(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members.namespaces.get(name).or_else(|| {
            self.core
                .as_ref()
                .and_then(|b| b.namespace_members.get(name))
        })
    }

    pub fn get_enum_members_local(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members
            .enums
            .get(name)
            .or_else(|| self.core.as_ref().and_then(|b| b.enum_members.get(name)))
    }

    pub fn get_class_methods_for(&self, name: &str) -> Option<&FxHashMap<Rc<str>, Type>> {
        self.class_methods
            .get(name)
            .or_else(|| self.core.as_ref().and_then(|b| b.class_methods.get(name)))
    }

    pub fn get_class_parent(&self, name: &str) -> Option<&str> {
        self.class_parents
            .get(name)
            .map(|s| s.as_ref())
            .or_else(|| {
                self.core
                    .as_ref()
                    .and_then(|b| b.class_parents.get(name))
                    .map(|s| s.as_ref())
            })
    }

    pub fn get_flattened_members(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members.flattened.get(name).or_else(|| {
            self.core
                .as_ref()
                .and_then(|b| b.flattened_members.get(name))
        })
    }

    /// Wire byte if `name` resolves (in global scope) to a free-function
    /// intrinsic import — e.g. `abs` imported from `std:math`. Lets bare
    /// `abs(x)` calls lower to `OpCode::Intrinsic`, the same path as the
    /// method form. Returns `None` for locals or non-intrinsic imports.
    pub fn intrinsic_import_wire(&self, name: &str) -> Option<u8> {
        let scope = self.scopes.get(self.global_scope);
        let atom = self.interner.get(name)?;
        let id = scope.resolve(atom, &self.scopes)?;
        self.arena.get(id).intrinsic_wire
    }

    /// The alias `name` declares in *this* module, if any.
    ///
    /// The `_local` suffix marks it as resolution-free, like its siblings
    /// above: following an alias declared elsewhere needs a [`BindView`].
    pub fn get_alias_node_local(&self, name: &str) -> Option<(Vec<String>, TypeNode)> {
        let scope = self.scopes.get(self.global_scope);
        let atom = self.interner.get(name)?;
        let id = scope.resolve(atom, &self.scopes)?;
        let sym = self.arena.get(id);
        let node = sym.alias_node.as_ref()?;
        Some((
            sym.type_params
                .iter()
                .map(|s| self.interner.resolve(*s).to_string())
                .collect(),
            *node.clone(),
        ))
    }

    pub fn has_named_type(&self, name: &str) -> bool {
        self.type_members.classes.contains_key(name)
            || self.type_members.interfaces.contains_key(name)
            || self.type_members.namespaces.contains_key(name)
            || self.type_members.enums.contains_key(name)
            || self
                .core
                .as_ref()
                .map(|b| {
                    b.class_members.contains_key(name)
                        || b.interface_members.contains_key(name)
                        || b.namespace_members.contains_key(name)
                        || b.enum_members.contains_key(name)
                })
                .unwrap_or(false)
    }
}

/// A bound module paired with the capability to follow its imports.
///
/// [`BindResult`] is **data**: cached as `Rc<BindResult>`, serialized to the
/// interface blobs on disk, shared between modules. Reaching another module is
/// a **capability**. Fusing the two — which is what `impl TypeContext for
/// BindResult` used to do — meant a serializable data structure carried the
/// power to read the filesystem, and could only exercise it through ambient
/// global state, because a `'static` cached value cannot hold a resolver.
///
/// Splitting them lets the same bound module be viewed under different
/// resolvers, and keeps the lifetime off the type that gets serialized.
pub struct BindView<'r> {
    pub bind: &'r BindResult,
    pub resolver: &'r dyn crate::module_resolver::ImportResolver,
}

impl<'r> BindView<'r> {
    pub fn new(
        bind: &'r BindResult,
        resolver: &'r dyn crate::module_resolver::ImportResolver,
    ) -> Self {
        Self { bind, resolver }
    }

    /// The bind for `origin`, when it names a module other than this one.
    fn foreign(&self, origin: Option<&str>) -> Option<Rc<BindResult>> {
        let origin = origin?;
        if origin == self.bind.source_file.as_ref() {
            return None;
        }
        self.resolver
            .module_bind(origin)
            .or_else(|| self.resolver.stdlib_bind(origin))
    }
}

impl TypeContext for BindView<'_> {
    fn interner(&self) -> Option<&varn_core::AtomInterner> {
        Some(&self.bind.interner)
    }

    fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>> {
        if let Some(rb) = self.foreign(origin) {
            return rb.get_interface_members_local(name).cloned();
        }
        self.bind.get_interface_members_local(name).cloned()
    }

    fn get_class_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(rb) = self.foreign(origin) {
            return rb.get_class_entry(name).map(|e| e.members.clone());
        }
        self.bind.get_class_entry(name).map(|e| e.members.clone())
    }

    fn get_namespace_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>> {
        if let Some(rb) = self.foreign(origin) {
            return rb.get_namespace_members_local(name).cloned();
        }
        self.bind.get_namespace_members_local(name).cloned()
    }

    fn get_enum_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(rb) = self.foreign(origin) {
            return rb.get_enum_members_local(name).cloned();
        }
        self.bind.get_enum_members_local(name).cloned()
    }

    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        let scope = self.bind.scopes.get(self.bind.global_scope);
        let atom = self.bind.interner.get(name)?;
        let id = scope.resolve(atom, &self.bind.scopes)?;
        self.bind.arena.get(id).ty.clone()
    }

    fn source_file(&self) -> Option<&str> {
        Some(self.bind.source_file.as_ref())
    }

    fn get_alias_node(&self, name: &str) -> Option<(Vec<String>, TypeNode)> {
        let scope = self.bind.scopes.get(self.bind.global_scope);
        let atom = self.bind.interner.get(name)?;
        let id = scope.resolve(atom, &self.bind.scopes)?;
        let sym = self.bind.arena.get(id);
        let node = sym.alias_node.as_ref()?;
        Some((
            sym.type_params
                .iter()
                .map(|s| self.bind.interner.resolve(*s).to_string())
                .collect(),
            *node.clone(),
        ))
    }

    fn resolve_type_alias(&self, name: &str, origin: Option<&str>) -> Option<Type> {
        if let Some(foreign_bind) = self.foreign(origin) {
            let foreign_view = BindView::new(&foreign_bind, self.resolver);
            return foreign_view.resolve_type_alias(name, None);
        }
        let scope = self.bind.scopes.get(self.bind.global_scope);
        let atom = self.bind.interner.get(name)?;
        let id = scope.resolve(atom, &self.bind.scopes)?;
        let sym = self.bind.arena.get(id);
        if sym.kind == crate::binder::SymbolKind::TypeAlias {
            return sym.ty.clone();
        }
        None
    }

    fn resolver(&self) -> Option<&dyn crate::module_resolver::ImportResolver> {
        Some(self.resolver)
    }

    fn get_extension_method(&self, type_name: &str, method_name: &str) -> Option<Type> {
        let mangled = self
            .bind
            .extensions
            .methods
            .get(type_name)?
            .get(method_name)?;
        self.resolve_symbol(mangled)
    }
}
