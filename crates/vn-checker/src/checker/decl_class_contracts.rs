use super::Checker;
use vn_core::ast::ClassMember;
use vn_core::{Diagnostic, ErrorCode};
use vn_sem::bind::BindResult;
use vn_sem::types::{ClassMemberInfo, ClassMemberKind};

impl<'r> Checker<'r> {
    pub(super) fn check_override_signature(
        &mut self,
        class: &str,
        member: &str,
        superclass_members: &[ClassMemberInfo],
        range: vn_core::SourceRange,
        bind: &BindResult,
    ) {
        let is_method = |m: &ClassMemberInfo| {
            m.name.as_ref() == member && m.kind == ClassMemberKind::Method && !m.is_static
        };
        let Some(inherited) = superclass_members
            .iter()
            .find(|m| is_method(m))
            .map(|m| m.ty)
        else {
            return;
        };
        let Some(own) = bind
            .get_class_entry(class)
            .and_then(|entry| entry.members.iter().find(|m| is_method(m)).map(|m| m.ty))
        else {
            return;
        };
        let inherited = match self.substitute_chain(class, inherited, bind) {
            Some((ty, true)) => ty,
            _ => {
                if self.lineage_is_generic(class, bind) {
                    return;
                }
                inherited
            }
        };
        let by_position = (
            self.by_type_param_position(inherited),
            self.by_type_param_position(own),
        );
        if self.types_compatible_cached(&by_position.0, &by_position.1, Some(bind)) {
            return;
        }
        let own_s = own.display(&self.ty_table, &bind.interner);
        let inherited_s = inherited.display(&self.ty_table, &bind.interner);
        self.emit(
            Diagnostic::error(
                ErrorCode::IncompatibleOverride,
                format!(
                    "member '{member}' is '{own_s}', which cannot stand in for the '{inherited_s}' it overrides"
                ),
            )
            .with_range(range),
        );
    }

    fn by_type_param_position(&mut self, ty: vn_sem::types::Type) -> vn_sem::types::Type {
        let vn_core::TypeKind::Fn(id) = self.ty_table.get(ty.0) else {
            return ty;
        };
        let declared = self.ty_table.get_function(id).type_params.clone();
        if declared.is_empty() {
            return ty;
        }
        let table = std::sync::Arc::make_mut(&mut self.ty_table);
        let mapping: rustc_hash::FxHashMap<vn_core::Atom, vn_sem::types::Type> = declared
            .iter()
            .enumerate()
            .map(|(at, name)| {
                let slot = vn_sem::types::Type::named(format!("<type parameter {at}>"), table);
                (vn_core::Atom::of(name), slot)
            })
            .collect();
        ty.map_generics(&mapping, table)
    }

    fn substitute_chain(
        &mut self,
        class: &str,
        inherited: vn_sem::types::Type,
        bind: &BindResult,
    ) -> Option<(vn_sem::types::Type, bool)> {
        use crate::checker_expressions::members::member_util::generic_mapping;
        let mut chain: Vec<vn_sem::bind::ClassParent> = Vec::new();
        let mut level = class.to_owned();
        for _ in 0..64 {
            let Some(parent) = bind.class_parents.get(level.as_str()) else {
                break;
            };
            if parent.origin.is_some() {
                return None;
            }
            chain.push(parent.clone());
            level = parent.name.to_string();
        }
        let mut current = inherited;
        let mut engaged = false;
        for parent in chain.iter().rev() {
            let mapping = generic_mapping(
                self.resolver,
                &parent.name,
                &parent.args,
                parent.origin.as_ref(),
                bind,
            );
            if mapping.is_empty() {
                continue;
            }
            engaged = true;
            let table = std::sync::Arc::make_mut(&mut self.ty_table);
            current = current.map_generics(&mapping, table);
        }
        Some((current, engaged))
    }

    pub(super) fn check_abstract_members(
        &mut self,
        c: &vn_core::ast::ClassDecl,
        superclass_members: &[ClassMemberInfo],
        bind: &BindResult,
    ) {
        let Some(id) = c.id else {
            return;
        };
        if c.modifiers.is_abstract {
            return;
        }
        let class = bind.interner.resolve(id);
        let mut settled: Vec<std::sync::Arc<str>> = c
            .body
            .iter()
            .filter_map(|member| match member {
                ClassMember::Method { key, .. }
                | ClassMember::Getter { key, .. }
                | ClassMember::Setter { key, .. }
                | ClassMember::Property { key, .. } => {
                    Some(std::sync::Arc::from(bind.interner.resolve(*key)))
                }
                ClassMember::Constructor { .. }
                | ClassMember::Destructor { .. }
                | ClassMember::StaticBlock { .. } => None,
            })
            .collect();
        for inherited in superclass_members {
            if settled.contains(&inherited.name) {
                continue;
            }
            settled.push(inherited.name.clone());
            if !inherited.is_abstract {
                continue;
            }
            self.emit(
                Diagnostic::error(
                    ErrorCode::AbstractMethodNotImplemented,
                    format!(
                        "class '{class}' does not implement the abstract member '{}'",
                        inherited.name
                    ),
                )
                .with_range(c.range),
            );
        }
    }

    pub(super) fn check_member_names(&mut self, c: &vn_core::ast::ClassDecl, bind: &BindResult) {
        let mut seen: Vec<(vn_core::Atom, bool, u8)> = Vec::new();
        for param in c.body.iter().flat_map(|member| match member {
            ClassMember::Constructor { params, .. } => params.as_slice(),
            ClassMember::Method { .. }
            | ClassMember::Property { .. }
            | ClassMember::Getter { .. }
            | ClassMember::Setter { .. }
            | ClassMember::Destructor { .. }
            | ClassMember::StaticBlock { .. } => &[],
        }) {
            let declares_property =
                param.modifiers.is_readonly || param.modifiers.visibility.is_some();
            let vn_core::ast::Pattern::Identifier { name, range, .. } = &param.pattern else {
                continue;
            };
            if !declares_property {
                continue;
            }
            if seen
                .iter()
                .any(|(other, is_static, _)| other == name && !*is_static)
            {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::DuplicateMemberName,
                        format!(
                            "the class already declares a member named '{}'",
                            bind.interner.resolve(*name)
                        ),
                    )
                    .with_range(*range),
                );
            }
            seen.push((*name, false, 0u8));
        }
        for member in &c.body {
            let (key, modifiers, range, role) = match member {
                ClassMember::Method {
                    key,
                    modifiers,
                    range,
                    ..
                }
                | ClassMember::Property {
                    key,
                    modifiers,
                    range,
                    ..
                } => (key, modifiers, range, 0u8),
                ClassMember::Getter {
                    key,
                    modifiers,
                    range,
                    ..
                } => (key, modifiers, range, 1u8),
                ClassMember::Setter {
                    key,
                    modifiers,
                    range,
                    ..
                } => (key, modifiers, range, 2u8),
                ClassMember::Constructor { .. }
                | ClassMember::Destructor { .. }
                | ClassMember::StaticBlock { .. } => continue,
            };
            let clash = seen.iter().any(|(name, is_static, other)| {
                name == key
                    && *is_static == modifiers.is_static
                    && (*other == role || *other == 0 || role == 0)
            });
            if clash {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::DuplicateMemberName,
                        format!(
                            "the class already declares a member named '{}'",
                            bind.interner.resolve(*key)
                        ),
                    )
                    .with_range(*range),
                );
            }
            seen.push((*key, modifiers.is_static, role));
        }
    }

    pub(super) fn check_implements(&mut self, c: &vn_core::ast::ClassDecl, bind: &BindResult) {
        let Some(id) = c.id else {
            return;
        };
        if c.implements.is_empty() || !c.type_params.is_empty() {
            return;
        }
        let class = bind.interner.resolve(id);
        let Some(instance) = bind
            .scopes
            .get(self.current_scope)
            .resolve(id, &bind.scopes)
            .and_then(|sid| bind.arena.get(sid).ty)
        else {
            return;
        };
        for promised in &c.implements {
            let contract = self.resolve_type_node_cached(promised, bind);
            let satisfied = contract.is_unresolved()
                || match self.class_contract_satisfied(&contract, &instance, bind) {
                    Some(verdict) => verdict,
                    None => self.types_compatible_cached(&contract, &instance, Some(bind)),
                };
            if satisfied {
                continue;
            }
            let contract_s = contract.display(&self.ty_table, &bind.interner);
            self.emit(
                Diagnostic::error(
                    ErrorCode::InterfaceNotSatisfied,
                    format!("class '{class}' declares that it implements '{contract_s}' but does not satisfy it"),
                )
                .with_range(*promised.range()),
            );
        }
    }

    fn class_contract_satisfied(
        &mut self,
        contract: &vn_sem::types::Type,
        instance: &vn_sem::types::Type,
        bind: &BindResult,
    ) -> Option<bool> {
        use crate::checker_expressions::members::member_util::generic_mapping;
        use vn_sem::bind::BindView;
        use vn_sem::types::TypeContext;
        let vn_core::TypeKind::Generic(name_atom, args_list, origin_atom) =
            self.ty_table.get(contract.0)
        else {
            return None;
        };
        let name = self.resolve_bind_atom(bind, name_atom);
        let origin = origin_atom.map(|o| self.resolve_bind_atom(bind, o));
        if origin
            .as_deref()
            .is_some_and(|o| o != bind.source_file.as_ref())
        {
            return None;
        }
        let view = BindView::new(bind, self.resolver);
        let members = view.get_class_members(&name, origin.as_deref())?;
        let args: Vec<vn_sem::types::Type> = self
            .ty_table
            .get_list(args_list)
            .iter()
            .map(|id| vn_sem::types::Type::resolved(*id))
            .collect();
        let mapping = generic_mapping(self.resolver, &name, &args, origin.as_ref(), bind);
        if mapping.is_empty() {
            return None;
        }
        let table = std::sync::Arc::make_mut(&mut self.ty_table);
        let object_members: Vec<_> = members
            .iter()
            .filter(|cm| {
                !cm.is_static
                    && matches!(
                        cm.kind,
                        ClassMemberKind::Method
                            | ClassMemberKind::Function
                            | ClassMemberKind::Property
                            | ClassMemberKind::Variable
                            | ClassMemberKind::Getter
                            | ClassMemberKind::Setter
                    )
            })
            .map(|cm| cm.as_object_member(table).map_generics(&mapping, table))
            .collect();
        let object = vn_sem::types::Type::interface(object_members, table);
        Some(self.types_compatible_cached(&object, instance, Some(bind)))
    }

    fn lineage_is_generic(&self, class: &str, bind: &BindResult) -> bool {
        use crate::checker_expressions::members::member_util::class_type_params;
        let mut name = class.to_owned();
        let mut origin = None;
        for _ in 0..64 {
            if !class_type_params(self.resolver, &name, origin.as_ref(), bind).is_empty() {
                return true;
            }
            match bind.class_parents.get(name.as_str()) {
                Some(parent) => {
                    origin = parent.origin.clone();
                    name = parent.name.to_string();
                }
                None => return false,
            }
        }
        true
    }
}
