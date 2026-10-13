use super::Checker;
use crate::checker::recorder::Recorder;
use std::collections::BTreeMap;
use std::sync::Arc;
use varn_core::ast::ExprId;
use varn_core::TypeKind;
use varn_sem::bind::{BindResult, ClassParent};
use varn_sem::output::{ForeignClass, InheritedField};
use varn_sem::types::{ClassMemberKind, FunctionParam, FunctionType, Type};

pub(crate) struct ForeignLevel<'a> {
    name: Arc<str>,
    owner: OwnerRef<'a>,
}

pub(crate) enum OwnerRef<'a> {
    Current(&'a BindResult),
    Foreign(std::sync::Arc<BindResult>),
}

impl<'a> OwnerRef<'a> {
    fn get(&self) -> &BindResult {
        match self {
            OwnerRef::Current(bind) => bind,
            OwnerRef::Foreign(bind) => bind,
        }
    }
}

impl Checker<'_> {
    pub(super) fn collect_foreign_inherited_fields(
        &mut self,
        bind: &BindResult,
    ) -> BTreeMap<Arc<str>, Vec<InheritedField>> {
        let mut classes: Vec<(Arc<str>, ClassParent)> = bind
            .class_parents
            .iter()
            .filter(|(_, p)| p.origin.is_some() || !bind.type_members.classes.contains_key(&p.name))
            .map(|(c, p)| (c.clone(), p.clone()))
            .collect();
        classes.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out = BTreeMap::new();
        for (class, parent) in classes {
            let mut fields = Vec::new();
            let mut visited = Vec::new();
            self.ancestor_fields(parent, bind, &mut fields, &mut visited);
            out.insert(class, fields);
        }
        out
    }

    pub(super) fn collect_foreign_classes<'t>(
        &mut self,
        bind: &BindResult,
        types: impl Iterator<Item = &'t Type>,
    ) -> Vec<ForeignClass> {
        let mut found: BTreeMap<(Arc<str>, Arc<str>), ForeignClass> = BTreeMap::new();
        for ty in types {
            let members = match self.ty_table.get(ty.0) {
                TypeKind::Union(list) => self.ty_table.get_list(list).to_vec(),
                TypeKind::Primitive(_)
                | TypeKind::Builtin(_)
                | TypeKind::Literal(_)
                | TypeKind::This
                | TypeKind::Array(_)
                | TypeKind::Intersection(_)
                | TypeKind::Tuple(_)
                | TypeKind::Named(..)
                | TypeKind::Generic(..)
                | TypeKind::TemplateLiteral(_)
                | TypeKind::Fn(_)
                | TypeKind::Object(_)
                | TypeKind::Typeof(_)
                | TypeKind::KeyOf(_)
                | TypeKind::IndexedAccess { .. }
                | TypeKind::Mapped { .. }
                | TypeKind::Conditional { .. }
                | TypeKind::Infer(_)
                | TypeKind::EnumVariant { .. }
                | TypeKind::TypePredicate { .. } => vec![ty.0],
            };
            for id in members {
                let (TypeKind::Named(name, Some(origin))
                | TypeKind::Generic(name, _, Some(origin))) = self.ty_table.get(id)
                else {
                    continue;
                };
                let origin = self.resolve_bind_atom(bind, origin);
                if origin.as_ref() == bind.source_file.as_ref() {
                    continue;
                }
                let name = self.resolve_bind_atom(bind, name);
                if varn_core::is_lang_type_name(name.as_ref()) {
                    continue;
                }
                let key = (name.clone(), origin.clone());
                if found.contains_key(&key) {
                    continue;
                }
                let owner = self
                    .resolver
                    .module_bind(&origin)
                    .or_else(|| self.resolver.stdlib_bind(&origin));
                let Some(owner) = owner else {
                    continue;
                };
                use varn_sem::bind::DeclKind;
                if !matches!(owner.classify_decl(name.as_ref()), Some(DeclKind::Class)) {
                    continue;
                }
                let chain = self.foreign_chain(name.as_ref(), OwnerRef::Foreign(owner.clone()));
                let mut fields = Vec::new();
                let mut methods = Vec::new();
                let constructor;
                for level in chain.iter().rev() {
                    let Some(entry) = level.owner.get().get_class_entry(&level.name) else {
                        continue;
                    };
                    for m in &entry.members {
                        if m.is_static {
                            continue;
                        }
                        match m.kind {
                            ClassMemberKind::Property | ClassMemberKind::Variable => {
                                if fields.iter().any(|f: &InheritedField| f.name == m.name) {
                                    continue;
                                }
                                fields.push(InheritedField {
                                    name: m.name.clone(),
                                    ty: self.reintern_foreign_ty(level.owner.get(), m.ty),
                                    optional: m.is_optional,
                                });
                            }
                            ClassMemberKind::Method | ClassMemberKind::Function => {
                                if !methods.contains(&m.name) {
                                    methods.push(m.name.clone());
                                }
                            }
                            ClassMemberKind::Constructor => {}
                            ClassMemberKind::Getter
                            | ClassMemberKind::Setter
                            | ClassMemberKind::Class
                            | ClassMemberKind::Interface
                            | ClassMemberKind::Namespace
                            | ClassMemberKind::Enum
                            | ClassMemberKind::Struct => {}
                        }
                    }
                }
                constructor =
                    Some(self.effective_ctor_ty(name.as_ref(), OwnerRef::Foreign(owner.clone())));
                methods.sort();
                let decorated = owner
                    .interner
                    .get(name.as_ref())
                    .and_then(|atom| {
                        owner
                            .scopes
                            .get(owner.global_scope)
                            .resolve(atom, &owner.scopes)
                    })
                    .is_none_or(|sid| owner.arena.get(sid).decorated);
                let import_binding = bind
                    .global_symbols()
                    .find(|s| {
                        !decorated
                            && s.origin_module
                                .is_some_and(|m| self.resolve_bind_atom(bind, m) == origin)
                            && s.original_name
                                .is_some_and(|n| self.resolve_bind_atom(bind, n) == name)
                    })
                    .map(|s| self.resolve_bind_atom(bind, s.name));
                found.insert(
                    key,
                    ForeignClass {
                        name,
                        origin,
                        fields,
                        methods,
                        constructor,
                        import_binding,
                    },
                );
            }
        }
        found.into_values().collect()
    }

    pub(crate) fn effective_ctor_ty(&mut self, name: &str, owner: OwnerRef<'_>) -> Type {
        for level in self.foreign_chain(name, owner) {
            let found = level
                .owner
                .get()
                .get_class_entry(&level.name)
                .and_then(|entry| {
                    entry
                        .members
                        .iter()
                        .find(|m| !m.is_static && m.kind == ClassMemberKind::Constructor)
                })
                .map(|m| self.reintern_foreign_ty(level.owner.get(), m.ty));
            if let Some(ty) = found {
                return ty;
            }
        }
        Type::fn_(
            FunctionType {
                params: Vec::new(),
                return_type: Type::Void.0,
                is_arrow: false,
                type_params: Vec::new(),
            },
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        )
    }

    pub(crate) fn synthesized_ctor_params(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        bind: &BindResult,
    ) -> Option<Vec<FunctionParam>> {
        let instance = self.infer_type(rec, expr, bind);
        let (name, origin) = match self.ty_table.get(instance.0) {
            varn_core::TypeKind::Generic(name, _, origin)
            | varn_core::TypeKind::Named(name, origin) => (
                self.resolve_bind_atom(bind, name),
                origin.map(|o| self.resolve_bind_atom(bind, o)),
            ),
            _ => return None,
        };
        let owner = match origin {
            Some(origin) if origin.as_ref() != bind.source_file.as_ref() => OwnerRef::Foreign(
                self.resolver
                    .module_bind(&origin)
                    .or_else(|| self.resolver.stdlib_bind(&origin))?,
            ),
            Some(_) | None => OwnerRef::Current(bind),
        };
        if owner.get().get_class_entry(&name).is_none() {
            return None;
        }
        let ty = self.effective_ctor_ty(&name, owner);
        let ty = match self.ty_table.get(instance.0) {
            varn_core::TypeKind::Generic(_, args, origin) => {
                let args: Vec<Type> = self
                    .ty_table
                    .get_list(args)
                    .iter()
                    .map(|id| Type::resolved(*id))
                    .collect();
                let origin = origin.map(|o| self.resolve_bind_atom(bind, o));
                let mapping = crate::checker_expressions::members::member_util::generic_mapping(
                    self.resolver,
                    &name,
                    &args,
                    origin.as_ref(),
                    bind,
                );
                ty.map_generics(&mapping, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
            }
            _ => ty,
        };
        Some(match self.ty_table.get(ty.0) {
            varn_core::TypeKind::Fn(fid) => self.ty_table.get_function(fid).params.clone(),
            _ => Vec::new(),
        })
    }

    fn foreign_chain<'a>(&self, name: &str, owner: OwnerRef<'a>) -> Vec<ForeignLevel<'a>> {
        let mut chain = vec![ForeignLevel {
            name: Arc::from(name),
            owner,
        }];
        let mut visited: Vec<(Arc<str>, Option<Arc<str>>)> = vec![(
            Arc::from(name),
            chain
                .first()
                .and_then(|level| level.owner.get().get_class_parent(name))
                .and_then(|p| p.origin.clone()),
        )];
        while let Some(level) = chain.last() {
            let parent = level.owner.get().get_class_parent(&level.name).cloned();
            let Some(parent) = parent else {
                break;
            };
            if visited.contains(&(parent.name.clone(), parent.origin.clone())) {
                break;
            }
            visited.push((parent.name.clone(), parent.origin.clone()));
            let next = parent.origin.as_deref().and_then(|o| {
                self.resolver
                    .module_bind(o)
                    .or_else(|| self.resolver.stdlib_bind(o))
            });
            let owner = match next {
                Some(bind) => OwnerRef::Foreign(bind),
                None => match &level.owner {
                    OwnerRef::Current(bind) => OwnerRef::Current(bind),
                    OwnerRef::Foreign(bind) => OwnerRef::Foreign(bind.clone()),
                },
            };
            chain.push(ForeignLevel {
                name: parent.name.clone(),
                owner,
            });
        }
        chain
    }

    fn ancestor_fields(
        &mut self,
        class: ClassParent,
        bind: &BindResult,
        out: &mut Vec<InheritedField>,
        visited: &mut Vec<ClassParent>,
    ) {
        if visited.contains(&class) {
            return;
        }
        visited.push(class.clone());
        let owner = class.origin.as_deref().and_then(|o| {
            self.resolver
                .module_bind(o)
                .or_else(|| self.resolver.stdlib_bind(o))
        });
        let declaring = owner.as_deref().unwrap_or(bind);
        let members = declaring
            .get_class_entry(&class.name)
            .map(|e| e.members.clone())
            .unwrap_or_default();
        let parent = declaring
            .get_class_parent(&class.name)
            .map(|p| ClassParent {
                name: p.name.clone(),
                origin: p.origin.clone().or_else(|| class.origin.clone()),
                args: Vec::new(),
            });
        if let Some(parent) = parent {
            self.ancestor_fields(parent, bind, out, visited);
        }
        for m in members {
            if m.is_static
                || !matches!(
                    m.kind,
                    ClassMemberKind::Property | ClassMemberKind::Variable
                )
                || out.iter().any(|f| f.name == m.name)
            {
                continue;
            }
            let ty = match &owner {
                Some(owner) => self.reintern_foreign_ty(owner, m.ty),
                None => m.ty,
            };
            out.push(InheritedField {
                name: m.name.clone(),
                ty,
                optional: m.is_optional,
            });
        }
    }
}
