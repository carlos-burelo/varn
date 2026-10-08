use std::sync::Arc;
use varn_core::ast::StructDecl;

use varn_sem::symbol::{Symbol, SymbolKind};
use varn_sem::types::Type;
use varn_sem::types::{ClassMemberInfo, ClassMemberKind};

impl<'r> super::super::Binder<'r> {
    pub(crate) fn bind_struct(&mut self, s: &StructDecl) {
        let id_rc: Arc<str> = Arc::from(self.interner.resolve(s.id));
        let struct_ty = Type::named_with_origin(
            id_rc.clone(),
            Some(Arc::from(self.source_file.as_ref())),
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        );
        let mut sym =
            Symbol::new(SymbolKind::Struct, s.id, s.range.start.line).with_type(struct_ty);
        sym.doc = s.doc.as_ref().map(|s| self.intern_local(s.as_str()));
        self.define(s.id, sym);

        let mut members = Vec::new();
        for field in &s.fields {
            let ty = self.resolve_type(&field.type_ann);
            let field_name_rc: Arc<str> = Arc::from(self.interner.resolve(field.name));

            let mut field_sym =
                Symbol::new(SymbolKind::Property, field.name, field.range.start.line).with_type(ty);
            field_sym.col = field.range.start.column;
            field_sym.offset = field.range.start.offset;
            field_sym.has_explicit_type = true;
            let symbol_id = self.arena.push(field_sym);

            members.push(ClassMemberInfo {
                name: field_name_rc,
                kind: ClassMemberKind::Property,
                is_async: false,
                is_generator: false,
                is_static: false,
                is_optional: false,
                line: field.range.start.line.saturating_sub(1),
                col: field.range.start.column,
                offset: field.range.start.offset,
                ty,
                members: Vec::new(),
                visibility: None,
                is_abstract: false,
                is_readonly: false,
                is_override: false,
                symbol_id: Some(symbol_id),
                ..Default::default()
            });
        }
        let struct_info = ClassMemberInfo {
            name: id_rc.clone(),
            kind: ClassMemberKind::Struct,
            is_async: false,
            is_generator: false,
            is_static: false,
            is_optional: false,
            line: s.range.start.line.saturating_sub(1),
            col: s.range.start.column,
            offset: s.range.start.offset,
            ty: Type::named_with_origin(
                id_rc.clone(),
                Some(Arc::from(self.source_file.as_ref())),
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            ),
            members,
            visibility: None,
            is_abstract: false,
            is_readonly: false,
            is_override: false,
            symbol_id: None,
            ..Default::default()
        };
        self.type_members.classes.insert(id_rc, struct_info);
    }
}
