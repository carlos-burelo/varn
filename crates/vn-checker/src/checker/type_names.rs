use super::Checker;
use std::sync::Arc;
use vn_core::ast::{ClassDecl, ClassMember, Param, TypeAliasDecl, TypeNode, TypeParam};
use vn_core::{Diagnostic, ErrorCode, TypeKind};
use vn_sem::bind::{BindResult, BindView};
use vn_sem::types::TypeContext;

enum AliasCycle {
    Plain,
    Expanding,
}

impl<'r> Checker<'r> {
    pub(crate) fn check_type_alias(&mut self, alias: &TypeAliasDecl, bind: &BindResult) {
        self.with_type_params(&alias.type_params, bind, |checker| {
            checker.check_type_names(&alias.alias, bind);
        });
        let name = bind.interner.resolve(alias.id).to_owned();
        let (code, detail) = match self.alias_reaches(&alias.alias, &name, bind, &mut Vec::new()) {
            Some(AliasCycle::Plain) => (
                ErrorCode::CircularTypeReference,
                "refers to itself with nothing between",
            ),
            Some(AliasCycle::Expanding) => (
                ErrorCode::InfiniteTypeExpansion,
                "expands without end: it refers to itself with other type arguments",
            ),
            None => return,
        };
        self.unknown_type_names.insert(alias.id);
        self.emit(
            Diagnostic::error(code, format!("type alias '{name}' {detail}"))
                .with_range(alias.alias.range),
        );
    }

    fn alias_reaches(
        &self,
        node: &TypeNode,
        target: &str,
        bind: &BindResult,
        seen: &mut Vec<String>,
    ) -> Option<AliasCycle> {
        match &node.kind {
            TypeKind::Named(name, _) | TypeKind::Generic(name, _, _) => {
                let text = self.resolve_bind_atom(bind, *name).to_string();
                let generic = matches!(node.kind, TypeKind::Generic(..));
                if text == target {
                    return Some(if generic {
                        AliasCycle::Expanding
                    } else {
                        AliasCycle::Plain
                    });
                }
                if seen.contains(&text) {
                    return None;
                }
                seen.push(text.clone());
                let view = BindView::new(bind, self.resolver).at(self.current_scope);
                let (_, body) = view.get_alias_node(&text)?;
                self.alias_reaches(&body, target, bind, seen)
            }
            TypeKind::Union(members) | TypeKind::Intersection(members) => members
                .iter()
                .find_map(|member| self.alias_reaches(member, target, bind, seen)),
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Record(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => None,
        }
    }

    pub(crate) fn check_type_names(&mut self, node: &TypeNode, bind: &BindResult) -> bool {
        if self.has_literal_container(node, bind) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::InvalidTypeAnnotation,
                    "literal types cannot be container element or key/value types",
                )
                .with_range(node.range),
            );
            return false;
        }
        match &node.kind {
            TypeKind::Named(name, _) => self.check_type_name(*name, node, bind),
            TypeKind::Generic(name, args, _) => {
                let mut known = self.check_type_name(*name, node, bind);
                for arg in args {
                    known &= self.check_type_names(arg, bind);
                }
                known
            }
            TypeKind::Array(inner) => self.check_type_names(inner, bind),
            TypeKind::Union(members)
            | TypeKind::Intersection(members)
            | TypeKind::Tuple(members) => {
                let mut known = true;
                for member in members {
                    known &= self.check_type_names(member, bind);
                }
                known
            }
            TypeKind::IndexedAccess { object, index } => {
                let primitive = match &object.kind {
                    TypeKind::Primitive(prim) => Some(prim.to_string()),
                    TypeKind::Named(name, _) => {
                        let text = self.resolve_bind_atom(bind, *name);
                        TypeKind::<(), (), (), (), (), ()>::of_primitive_name(&text)
                            .is_some()
                            .then(|| text.to_string())
                    }
                    TypeKind::Literal(_)
                    | TypeKind::This
                    | TypeKind::Array(_)
                    | TypeKind::Union(_)
                    | TypeKind::Intersection(_)
                    | TypeKind::Tuple(_)
                    | TypeKind::Generic(..)
                    | TypeKind::TemplateLiteral(_)
                    | TypeKind::Fn(_)
                    | TypeKind::Record(_)
                    | TypeKind::Interface(_)
                    | TypeKind::Typeof(_)
                    | TypeKind::KeyOf(_)
                    | TypeKind::IndexedAccess { .. }
                    | TypeKind::Mapped { .. }
                    | TypeKind::Conditional { .. }
                    | TypeKind::Infer(_)
                    | TypeKind::EnumVariant { .. }
                    | TypeKind::TypePredicate { .. } => None,
                };
                if let Some(prim) = primitive {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::InvalidTypeOperator,
                            format!(
                                "'{prim}' cannot be indexed in a type: fixed-size arrays are not supported, use '{prim}[]'"
                            ),
                        )
                        .with_range(node.range),
                    );
                    return false;
                }
                self.check_type_names(object, bind) & self.check_type_names(index, bind)
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Record(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => true,
        }
    }

    fn has_literal_container(&mut self, node: &TypeNode, bind: &BindResult) -> bool {
        let view = BindView::new(bind, self.resolver).at(self.current_scope);
        let root = vn_sem::resolve::resolve_type_node(
            node,
            Some(&view),
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        );
        let table = &self.ty_table;
        let mut pending = vec![root];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(ty) = pending.pop() {
            let ty = vn_sem::resolve::expand_alias(ty, Some(&view), table);
            if !visited.insert(ty.0) {
                continue;
            }
            if let Some(
                kind @ (vn_core::BuiltinType::Array
                | vn_core::BuiltinType::Map
                | vn_core::BuiltinType::Set),
            ) = ty.builtin_of(table)
            {
                let count = if kind == vn_core::BuiltinType::Map {
                    2
                } else {
                    1
                };
                for i in 0..count {
                    let arg =
                        vn_sem::resolve::expand_alias(ty.builtin_arg(i, table), Some(&view), table);
                    if matches!(table.get(arg.0), TypeKind::Literal(_)) {
                        return true;
                    }
                }
            }
            pending.extend(ty.components(table));
        }
        false
    }

    fn check_type_name(&mut self, name: vn_core::Atom, node: &TypeNode, bind: &BindResult) -> bool {
        let text = self.resolve_bind_atom(bind, name);
        if TypeKind::<(), (), (), (), (), ()>::of_primitive_name(&text).is_some()
            || text.as_ref() == vn_core::well_known::RECORD
            || self.active_type_params.contains(text.as_ref())
        {
            return true;
        }
        let scope = bind.scopes.get(self.current_scope);
        if scope.resolve(name, &bind.scopes).is_some() {
            return true;
        }
        let view = BindView::new(bind, self.resolver).at(self.current_scope);
        if view.resolve_symbol(&text).is_some() || view.get_alias_node(&text).is_some() {
            return true;
        }
        let table = std::sync::Arc::make_mut(&mut self.ty_table);
        if vn_sem::resolve::qualified_type(&text, Some(&view), table).is_some() {
            return true;
        }
        self.unknown_type_names.insert(name);
        self.emit(
            Diagnostic::error(ErrorCode::UnknownSymbol, format!("unknown type: {text}"))
                .with_range(node.range),
        );
        false
    }

    fn with_type_params(
        &mut self,
        type_params: &[TypeParam],
        bind: &BindResult,
        body: impl FnOnce(&mut Self),
    ) {
        let mut injected: Vec<Arc<str>> = Vec::new();
        for tp in type_params {
            let name: Arc<str> = Arc::from(bind.interner.resolve(tp.name));
            if self.active_type_params.insert(name.clone()) {
                injected.push(name);
            }
        }
        for tp in type_params {
            if let Some(constraint) = &tp.constraint {
                self.check_type_names(constraint, bind);
            }
            if let Some(default) = &tp.default {
                self.check_type_names(default, bind);
            }
        }
        body(self);
        for name in injected {
            self.active_type_params.remove(name.as_ref());
        }
    }

    pub(crate) fn check_signature_type_names(
        &mut self,
        type_params: &[TypeParam],
        params: &[Param],
        return_type: Option<&TypeNode>,
        bind: &BindResult,
    ) {
        self.with_type_params(type_params, bind, |checker| {
            for param in params {
                if let Some(ann) = &param.type_ann {
                    checker.check_type_names(ann, bind);
                }
            }
            if let Some(ret) = return_type {
                checker.check_type_names(ret, bind);
            }
        });
    }

    pub(crate) fn check_class_type_names(&mut self, class: &ClassDecl, bind: &BindResult) {
        self.with_type_params(&class.type_params, bind, |checker| {
            if let Some(primary) = &class.primary_params {
                checker.check_signature_type_names(&[], primary, None, bind);
            }
            for member in &class.body {
                match member {
                    ClassMember::Constructor { params, .. } => {
                        checker.check_signature_type_names(&[], params, None, bind);
                    }
                    ClassMember::Method {
                        type_params,
                        params,
                        return_type,
                        ..
                    } => checker.check_signature_type_names(
                        type_params,
                        params,
                        return_type.as_ref(),
                        bind,
                    ),
                    ClassMember::Property { type_ann, .. } => {
                        if let Some(ann) = type_ann {
                            checker.check_type_names(ann, bind);
                        }
                    }
                    ClassMember::Getter { return_type, .. } => {
                        if let Some(ret) = return_type {
                            checker.check_type_names(ret, bind);
                        }
                    }
                    ClassMember::Setter { param, .. } => {
                        checker.check_signature_type_names(
                            &[],
                            std::slice::from_ref(param),
                            None,
                            bind,
                        );
                    }
                    ClassMember::Destructor { .. } | ClassMember::StaticBlock { .. } => {}
                }
            }
        });
    }
}
