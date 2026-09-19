use std::rc::Rc;
use varn_core::ast::{Pattern, SumTypeDecl};

use crate::symbol::{Symbol, SymbolKind};
use crate::types::Type;

impl<'r> super::super::Binder<'r> {
    pub(crate) fn bind_pattern(
        &mut self,
        pattern: &Pattern,
        kind: SymbolKind,
        line: u32,
        doc: Option<String>,
        ty: Option<Type>,
    ) {
        match pattern {
            Pattern::Identifier {
                name,
                type_ann,
                range,
                ..
            } => {
                let mut sym = Symbol::new(kind, *name, line);
                sym.doc = doc.map(|d| self.interner.intern(&d));
                sym.col = range.start.column;
                sym.offset = range.start.offset;
                sym.has_explicit_type = type_ann.is_some();
                if let Some(ann) = type_ann {
                    sym.ty = Some(self.resolve_type(ann));
                } else {
                    sym.ty = ty;
                }
                self.define(*name, sym);
            }
            Pattern::Array { elements, rest, .. } => {
                let elem_ty = ty.as_ref().and_then(|t| match *self.ty_table.get(t.0) {
                    varn_core::TypeKind::Array(inner) => Some(Type(inner, false)),
                    varn_core::TypeKind::Generic(name, args, _)
                        if self.interner.resolve(name) == varn_core::IntrinsicType::Array.as_str()
                            && self.ty_table.get_list(args).len() == 1 =>
                    {
                        Some(Type(self.ty_table.get_list(args)[0], false))
                    }
                    _ => None,
                });
                for el in elements.iter().flatten() {
                    self.bind_pattern(&el.pattern, kind, line, doc.clone(), elem_ty.clone());
                }
                if let Some(r) = rest {
                    self.bind_pattern(r, kind, line, doc.clone(), ty);
                }
            }
            Pattern::Object {
                properties, rest, ..
            } => {
                for prop in properties {
                    let mut prop_kind = kind;
                    let key_str = self.interner.resolve(prop.key).to_string();
                    let ty_kind = ty.map(|t| *self.ty_table.get(t.0));
                    let prop_ty = match ty_kind {
                        Some(varn_core::TypeKind::Object(mid)) => {
                            self.ty_table.get_object_members(mid).to_vec().iter().find_map(|m| {
                                match m {
                                    crate::types::ObjectTypeMember::Property {
                                        name, ty, ..
                                    } if name.as_ref() == key_str => Some(Type(*ty, false)),
                                    crate::types::ObjectTypeMember::Method {
                                        name,
                                        params,
                                        return_type,
                                        is_arrow,
                                        ..
                                    } if name.as_ref() == key_str => {
                                        Some(crate::types::Type::fn_(
                                            crate::types::FunctionType {
                                                params: params.clone(),
                                                return_type: *return_type,
                                                is_arrow: *is_arrow,
                                                type_params: vec![],
                                            },
                                            &mut self.ty_table,
                                        ))
                                    }
                                    _ => None,
                                }
                            })
                        }
                        Some(varn_core::TypeKind::Named(name_atom, origin_atom))
                        | Some(varn_core::TypeKind::Generic(name_atom, _, origin_atom)) => {
                            let name: Rc<str> = Rc::from(self.interner.resolve(name_atom));
                            let origin: Option<Rc<str>> =
                                origin_atom.map(|o| Rc::from(self.interner.resolve(o)));
                            self.get_class_members(name.as_ref(), origin.as_deref())
                                .or_else(|| {
                                    self.get_interface_members(name.as_ref(), origin.as_deref())
                                })
                                .and_then(|members| {
                                    members
                                        .iter()
                                        .find(|m| m.name.as_ref() == key_str)
                                        .map(|m| m.ty.clone())
                                })
                                .or_else(|| {
                                    if name.as_ref() != "*" {
                                        return None;
                                    }
                                    let origin_path = origin.as_deref()?;
                                    let mut visiting = vec![self.source_file.to_string()];
                                    let exports =
                                        self.resolver.module_exports(origin_path, &mut visiting);
                                    if let Some(sym) = exports.get(key_str.as_str()) {
                                        prop_kind = sym.kind;
                                        sym.ty.clone()
                                    } else {
                                        let has_ns = self
                                            .type_members
                                            .namespaces
                                            .get(key_str.as_str())
                                            .and_then(|members| members.first())
                                            .is_some();
                                        if has_ns {
                                            prop_kind = SymbolKind::Namespace;
                                            Some(Type::named_with_origin(
                                                key_str.clone(),
                                                Some(Rc::from(origin_path)),
                                                self.resolver,
                                                &mut self.ty_table,
                                            ))
                                        } else {
                                            None
                                        }
                                    }
                                })
                        }
                        _ => None,
                    };
                    self.bind_pattern(&prop.value, prop_kind, line, doc.clone(), prop_ty);
                }
                if let Some(r) = rest {
                    // El rest-object NO hereda el tipo del objeto fuente: sus
                    // miembros son solo los NO excluidos, y el runtime devuelve
                    // `null` para los excluidos. Tipar `restObj.alpha` como el
                    // `int` del fuente es afirmar algo que el runtime no cumple
                    // (misma mentira que K1 corrigió para `char`): se tipa
                    // dinámico para que los accesos a excluidos sigan siendo
                    // legales y viajen por un registro DYN, no por uno `Int`.
                    let _ = &ty;
                    self.bind_pattern(r, kind, line, doc.clone(), None);
                }
            }
            Pattern::Assignment { left, right, .. } => {
                let resolved_ty = if ty.is_none() || ty.as_ref().is_some_and(|t| t.is_dynamic()) {
                    let inferred = self.infer_expr_type_self(*right);
                    if inferred.is_dynamic() {
                        ty
                    } else {
                        Some(inferred)
                    }
                } else {
                    ty
                };
                self.bind_pattern(left, kind, line, doc, resolved_ty);
            }
            Pattern::Rest { argument, .. } => {
                self.bind_pattern(argument, kind, line, doc, ty);
            }
        }
    }

    pub(crate) fn bind_sum_type(&mut self, t: &SumTypeDecl) {
        let id_rc: Rc<str> = Rc::from(self.interner.resolve(t.id));
        let alias_ty = Type::named_with_origin(
            id_rc.clone(),
            Some(Rc::from(self.source_file.as_ref())),
            self.resolver,
            &mut self.ty_table,
        );
        let mut pe_sym =
            Symbol::new(SymbolKind::TypeAlias, t.id, t.range.start.line).with_type(alias_ty);
        // Expose the alias' generic parameters so consumers (e.g. match-variant
        // payload typing) can substitute them with concrete type arguments.
        pe_sym.type_params = t.type_params.iter().map(|tp| tp.name).collect();
        self.define(t.id, pe_sym);

        // `sum_type_variants`/`sum_variant_parent`/`sum_variant_fields` are
        // consumed well outside this cluster (`emit::tables`,
        // `checker_expressions::members::{member_exists,member_type}`,
        // `checker_expressions::patterns`, `checker_expressions::check::exhaustiveness`)
        // and stay `Rc<str>`-keyed; text is resolved from the `Atom` here at
        // the point of insertion rather than migrating those consumers too.
        let mut variant_names = Vec::new();

        for v in &t.variants {
            let variant_rc: Rc<str> = Rc::from(self.interner.resolve(v.name));
            variant_names.push(variant_rc.clone());

            let fields: Vec<(Rc<str>, Type)> = v
                .fields
                .iter()
                .map(|f| {
                    let ty = self.resolve_type(&f.ty);
                    (Rc::from(self.interner.resolve(f.name)), ty)
                })
                .collect();

            self.sum_variant_parent
                .insert(variant_rc.clone(), id_rc.clone());
            self.sum_variant_fields
                .insert(variant_rc.clone(), fields.clone());

            if v.fields.is_empty() {
                let variant_ty = Type::named_with_origin(
                    id_rc.clone(),
                    Some(Rc::from(self.source_file.as_ref())),
                    self.resolver,
                    &mut self.ty_table,
                );
                let sym = Symbol::new(SymbolKind::Const, v.name, v.range.start.line)
                    .with_type(variant_ty);
                self.define(v.name, sym);
            } else {
                let params: Vec<crate::types::FunctionParam> = fields
                    .iter()
                    .map(|(fname, fty)| crate::types::FunctionParam {
                        name: Some(fname.clone()),
                        ty: fty.0,
                        optional: false,
                        is_rest: false,
                    })
                    .collect();
                let ret_ty = Type::named_with_origin(
                    id_rc.clone(),
                    Some(Rc::from(self.source_file.as_ref())),
                    self.resolver,
                    &mut self.ty_table,
                );
                let fn_ty = Type::fn_(
                    crate::types::FunctionType {
                        params,
                        return_type: ret_ty.0,
                        is_arrow: false,
                        type_params: t
                            .type_params
                            .iter()
                            .map(|tp| Rc::from(self.interner.resolve(tp.name)))
                            .collect(),
                    },
                    &mut self.ty_table,
                );
                let sym =
                    Symbol::new(SymbolKind::Function, v.name, v.range.start.line).with_type(fn_ty);
                self.define(v.name, sym);
            }
        }

        self.sum_type_variants.insert(id_rc, variant_names);
    }
}
