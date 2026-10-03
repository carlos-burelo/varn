use super::Binder;
use crate::binder::ClassMemberInfo;
use crate::binder::PendingEnrich;
use crate::symbol::SymbolKind;
use rustc_hash::FxHashSet;
use std::sync::Arc;
use varn_core::ast::{ClassDecl, ClassMember};

impl<'r> Binder<'r> {
    pub(super) fn bind_class_bodies(&mut self, c: &ClassDecl, name_atom: varn_core::Atom) {
        for member in &c.body {
            match member {
                ClassMember::Constructor {
                    params,
                    body,
                    range,
                    ..
                } => {
                    self.bind_inline_function(&[], params, None, *body, range);
                }
                ClassMember::Method {
                    key,
                    type_params,
                    params,
                    return_type,
                    body: Some(body),
                    range,
                    modifiers,
                    ..
                } => {
                    if return_type.is_none() && !modifiers.is_abstract {
                        self.pending_enrich.push(PendingEnrich::Method {
                            class_name: name_atom,
                            key: *key,
                            body: *body,
                            is_async: modifiers.is_async,
                        });
                    }
                    self.bind_inline_function(
                        type_params,
                        params,
                        return_type.as_ref(),
                        *body,
                        range,
                    );
                }
                ClassMember::Getter {
                    key,
                    return_type,
                    body: Some(body),
                    ..
                } => {
                    if return_type.is_none() {
                        self.pending_enrich.push(PendingEnrich::Getter {
                            class_name: name_atom,
                            key: *key,
                            body: *body,
                        });
                    }
                    self.escape_all_open_array_candidates();
                    self.bind_stmt(*body);
                }
                ClassMember::Setter {
                    key,
                    param,
                    body: Some(body),
                    range,
                    ..
                } => {
                    self.pending_enrich.push(PendingEnrich::Setter {
                        class_name: name_atom,
                        key: *key,
                        body: *body,
                    });
                    self.escape_all_open_array_candidates();
                    self.bind_stmt(*body);
                    let ty = param.type_ann.as_ref().map(|ann| self.resolve_type(ann));
                    self.bind_pattern(
                        &param.pattern,
                        SymbolKind::Parameter,
                        range.start.line,
                        None,
                        ty,
                        param.type_ann.is_some(),
                    );
                }
                ClassMember::Property {
                    init: Some(init), ..
                } => {
                    self.bind_expr(*init);
                }
                _ => {}
            }
        }
    }

    pub(super) fn mark_optional_fields(&mut self, c: &ClassDecl, members: &mut [ClassMemberInfo]) {
        let declared_ctor = c.body.iter().find_map(|m| match m {
            ClassMember::Constructor { body, .. } => Some(body),
            _ => None,
        });
        let candidate_fields: Vec<Arc<str>> = c
            .body
            .iter()
            .filter_map(|m| match m {
                ClassMember::Property {
                    key,
                    init: None,
                    modifiers,
                    ..
                } if !modifiers.is_static => Some(Arc::from(self.interner.resolve(*key))),
                _ => None,
            })
            .collect();
        if !candidate_fields.is_empty() && !c.modifiers.is_declare {
            let guaranteed: FxHashSet<Arc<str>> = match declared_ctor {
                Some(body) => super::definite_field_assignment::fields_assigned_on_every_path(
                    *body,
                    self.ast_arena,
                    &self.interner,
                ),
                None => FxHashSet::default(),
            };
            for field in &candidate_fields {
                if !guaranteed.contains(field) {
                    if let Some(m) = members.iter_mut().find(|m| &m.name == field) {
                        m.is_optional = true;
                    }
                }
            }
        }
    }
}
