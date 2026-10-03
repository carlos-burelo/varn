use super::type_inference::pattern_lead_name;
use super::Binder;
use crate::symbol::{Symbol, SymbolKind};
use crate::types::{ClassMemberInfo, ClassMemberKind, FunctionParam, FunctionType, Type};
use std::sync::Arc;
use varn_core::ast::ClassDecl;

impl<'r> Binder<'r> {
    pub(super) fn bind_primary_constructor(
        &mut self,
        c: &ClassDecl,
        members: &mut Vec<ClassMemberInfo>,
    ) {
        let Some(primary_params) = &c.primary_params else {
            return;
        };
        let ps: Vec<FunctionParam> = primary_params
            .iter()
            .map(|p| self.function_param(p, super::binding_types::ParamSite::Declared))
            .collect();

        let fn_ty = Type::fn_(
            FunctionType {
                params: ps,
                return_type: Type::Void.0,
                is_arrow: false,
                type_params: vec![],
            },
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        );

        let ctor_atom = self.intern_local("constructor");
        let mut sym =
            Symbol::new(SymbolKind::Method, ctor_atom, c.range.start.line).with_type(fn_ty);
        sym.col = c.range.start.column;
        sym.offset = c.range.start.offset;
        let symbol_id = self.arena.push(sym);

        members.push(ClassMemberInfo {
            name: Arc::from("constructor"),
            kind: ClassMemberKind::Constructor,
            is_async: false,
            is_generator: false,
            is_static: false,
            is_optional: false,
            line: c.range.start.line.saturating_sub(1),
            col: c.range.start.column,
            offset: c.range.start.offset,
            ty: fn_ty,
            members: Vec::new(),
            visibility: None,
            is_abstract: false,
            is_readonly: false,
            is_override: false,
            symbol_id: Some(symbol_id),
            ..Default::default()
        });

        for p in primary_params {
            let name_str = pattern_lead_name(&p.pattern, &self.interner).to_owned();
            let key_rc: Arc<str> = Arc::from(name_str.as_str());
            let key_atom = self.intern_local(&name_str);
            let ty = self.param_type(p, super::binding_types::ParamSite::Declared);

            let mut sym =
                Symbol::new(SymbolKind::Property, key_atom, p.range.start.line).with_type(ty);
            sym.col = p.range.start.column;
            sym.offset = p.range.start.offset;
            sym.has_explicit_type = p.type_ann.is_some();
            let symbol_id = self.arena.push(sym);

            members.push(ClassMemberInfo {
                name: key_rc,
                kind: ClassMemberKind::Property,
                is_async: false,
                is_generator: false,
                is_static: false,
                is_optional: false,
                line: p.range.start.line.saturating_sub(1),
                col: p.range.start.column,
                offset: p.range.start.offset,
                ty,
                members: Vec::new(),
                visibility: p.modifiers.visibility,
                is_abstract: false,
                is_readonly: p.modifiers.is_readonly,
                is_override: false,
                symbol_id: Some(symbol_id),
                ..Default::default()
            });
        }
    }
}
