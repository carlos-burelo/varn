use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_checker::{SymbolKind, Type};
use varn_core::{IntrinsicType, TokenKind, TypeKind, TypeTag};

use super::{ChainResult, DocumentState, MemberKind, MemberRecord};

fn is_expression_keyword(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Await | TokenKind::Yield)
}

fn is_identifier_like_for_hover(tokens: &[super::TokenRecord], idx: usize) -> bool {
    let tok = &tokens[idx];
    if !(tok.kind == TokenKind::Identifier || tok.kind.can_be_identifier()) {
        return false;
    }
    if !is_expression_keyword(tok.kind) {
        return true;
    }

    idx.checked_sub(1)
        .and_then(|j| tokens.get(j))
        .is_some_and(|prev| prev.kind == TokenKind::Dot || prev.kind == TokenKind::QuestionDot)
}

impl DocumentState {
    pub fn offset_at_line_col(&self, line: u32, col: u32) -> u32 {
        let mut curr_line = 0;
        let mut curr_col = 0;
        let bytes = self.source.as_bytes();
        for (offset, &b) in bytes.iter().enumerate() {
            if curr_line == line && curr_col == col {
                return offset as u32;
            }
            if b == b'\n' {
                curr_line += 1;
                curr_col = 0;
            } else if b != b'\r' {
                curr_col += 1;
            }
        }
        bytes.len() as u32
    }

    pub(crate) fn expr_info_at_token(
        &self,
        tok: &super::TokenRecord,
    ) -> Option<&varn_checker::ExprInfo> {
        if let Some(info) = self.db.expr_types.get(&tok.offset) {
            return Some(info);
        }
        for (&offset, info) in &self.db.expr_types {
            if offset >= tok.offset && offset < tok.offset + tok.length {
                return Some(info);
            }
        }
        None
    }

    pub fn resolve_chain_at(&self, line: u32, col: u32) -> Option<ChainResult> {
        let ident_idx = self.tokens.iter().enumerate().position(|(idx, t)| {
            t.line == line
                && is_identifier_like_for_hover(&self.tokens, idx)
                && t.col <= col
                && col < t.col + t.length
        })?;
        let target_tok = &self.tokens[ident_idx];

        let is_member_access = ident_idx >= 2
            && matches!(
                self.tokens[ident_idx - 1].kind,
                TokenKind::Dot | TokenKind::QuestionDot
            );

        if is_member_access {
            let obj_tok = &self.tokens[ident_idx - 2];
            let mut resolved_info = self.expr_info_at_token(obj_tok);

            if resolved_info.is_none() && obj_tok.kind == TokenKind::RParen {
                let mut depth = 0;
                let mut found_idx = None;
                for j in (0..ident_idx - 1).rev() {
                    if self.tokens[j].kind == TokenKind::RParen {
                        depth += 1;
                    } else if self.tokens[j].kind == TokenKind::LParen {
                        depth -= 1;
                        if depth == 0 {
                            found_idx = Some(j);
                            break;
                        }
                    }
                }
                if let Some(lparen_idx) = found_idx {
                    if lparen_idx >= 1 {
                        let callee_tok = &self.tokens[lparen_idx - 1];
                        let mut start_tok = callee_tok;
                        if lparen_idx >= 2 && self.tokens[lparen_idx - 2].kind == TokenKind::New {
                            start_tok = &self.tokens[lparen_idx - 2];
                        }
                        resolved_info = self.expr_info_at_token(start_tok);
                    }
                }
            }

            if resolved_info.is_none() && obj_tok.kind == TokenKind::RBracket {
                let mut depth = 0;
                let mut found_idx = None;
                for j in (0..ident_idx - 1).rev() {
                    if self.tokens[j].kind == TokenKind::RBracket {
                        depth += 1;
                    } else if self.tokens[j].kind == TokenKind::LBracket {
                        depth -= 1;
                        if depth == 0 {
                            found_idx = Some(j);
                            break;
                        }
                    }
                }
                if let Some(lbrack_idx) = found_idx {
                    if lbrack_idx >= 1 {
                        let array_tok = &self.tokens[lbrack_idx - 1];
                        resolved_info = self.expr_info_at_token(array_tok);
                    }
                }
            }

            if let Some(info) = resolved_info {
                let mut base_type = info.ty.clone();
                if base_type.is_nullable() {
                    base_type = base_type.non_nullified();
                }

                let parent_name = type_name_str(&base_type).unwrap_or_else(|| {
                    literal_primitive_type(obj_tok.kind)
                        .map(str::to_owned)
                        .unwrap_or_else(|| obj_tok.lexeme.clone())
                });

                let mut res_opt = self.find_chain_result(&base_type, &target_tok.lexeme);
                if res_opt.is_none() && base_type.is_dynamic() {
                    let virtual_type = Type::named(parent_name.clone());
                    res_opt = self.find_chain_result(&virtual_type, &target_tok.lexeme);
                }

                if let Some(mut res) = res_opt {
                    if let Some(target_info) = self.expr_info_at_token(target_tok) {
                        if !target_info.ty.is_dynamic() {
                            match &mut res {
                                ChainResult::Member { member, .. } => {
                                    let mut new_member = (*member).clone();
                                    new_member.ty = target_info.ty.clone();
                                    enrich_member_record(&mut new_member);
                                    return Some(ChainResult::DynamicMember {
                                        member: new_member,
                                        parent_name,
                                    });
                                }
                                ChainResult::DynamicMember { member, .. } => {
                                    member.ty = target_info.ty.clone();
                                    enrich_member_record(member);
                                }
                                _ => {}
                            }
                        }
                    }
                    return Some(res);
                }
            }
            return None;
        }

        if let Some(info) = self.expr_info_at_token(target_tok) {
            if let Some(sid) = info.symbol_id {
                if let Some(sym) = self.symbols.iter().find(|s| s.symbol_id == Some(sid)) {
                    return Some(ChainResult::Symbol(sym));
                }
            }
        }

        if let Some((sid, _ty)) = self.db.resolve_at(&target_tok.lexeme, target_tok.offset) {
            if let Some(sym) = self.symbols.iter().find(|s| s.symbol_id == Some(sid)) {
                return Some(ChainResult::Symbol(sym));
            }
        }

        if let Some(sym) = self.symbols.iter().find(|s| s.name == target_tok.lexeme) {
            return Some(ChainResult::Symbol(sym));
        }

        None
    }

    fn find_chain_result(&self, ty: &Type, name: &str) -> Option<ChainResult> {
        let type_name = type_name_str(ty)?;
        let mut is_enum = self
            .symbols
            .iter()
            .any(|s| s.name == type_name && s.kind == SymbolKind::Enum);
        if !is_enum {
            let origin = match &ty.0 {
                TypeKind::Named(_, o) | TypeKind::Generic(_, _, o) => {
                    o.as_ref().map(|s| s.as_ref())
                }
                _ => None,
            };
            if let Some(origin_mod) = origin {
                if let Some(rb) = varn_checker::module_resolver::resolve_module_bind_ref(origin_mod)
                    .or_else(|| {
                        varn_checker::module_resolver::resolve_stdlib_module_bind_ref(origin_mod)
                    })
                {
                    is_enum = rb.get_enum_members_local(&type_name).is_some();
                }
            }
        }
        if !is_enum {
            for import_path in &self.import_paths {
                let rb = varn_checker::module_resolver::resolve_module_bind_ref(import_path)
                    .or_else(|| {
                        varn_checker::module_resolver::resolve_stdlib_module_bind_ref(import_path)
                    });
                if let Some(ref_rb) = rb {
                    if ref_rb.get_enum_members_local(&type_name).is_some() {
                        is_enum = true;
                        break;
                    }
                }
            }
        }
        if is_enum && matches!(name, "rawValue" | "name" | "__tag" | "__variant_name__") {
            let prop_ty = if name == "rawValue" || name == "__tag" {
                varn_checker::types::Type::named("int")
            } else {
                varn_checker::types::Type::named("str")
            };
            return Some(ChainResult::DynamicMember {
                member: MemberRecord {
                    name: name.to_owned(),
                    type_str: prop_ty.to_string(),
                    params_str: String::new(),
                    is_static: false,
                    is_optional: false,
                    kind: MemberKind::Property,
                    is_arrow: false,
                    is_async: false,
                    is_generator: false,
                    line: u32::MAX,
                    col: u32::MAX,
                    init_value: String::new(),
                    ty: prop_ty,
                    symbol_id: None,
                    members: Vec::new(),
                },
                parent_name: type_name,
            });
        }
        match &ty.0 {
            TypeKind::Named(_, _)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Intrinsic(_)
            | TypeKind::Array(_) => {
                let mapping = if let TypeKind::Generic(gname, type_args, _) = &ty.0 {
                    self.build_generic_mapping_lsp(gname, type_args)
                } else {
                    FxHashMap::default()
                };
                if let Some(sym) = self.symbols.iter().find(|s| &s.name == &type_name) {
                    if let Some(m) = self.find_member_recursive(&sym.members, name) {
                        if mapping.is_empty() {
                            return Some(ChainResult::Member {
                                member: m,
                                parent_name: sym.name.clone(),
                            });
                        }
                        return Some(ChainResult::DynamicMember {
                            member: substitute_member_record(m, &mapping),
                            parent_name: sym.name.clone(),
                        });
                    }
                }
                if let Some(m) = self.find_extension_member(&type_name, name) {
                    if mapping.is_empty() {
                        return Some(ChainResult::Member {
                            member: m,
                            parent_name: type_name.clone(),
                        });
                    }
                    return Some(ChainResult::DynamicMember {
                        member: substitute_member_record(m, &mapping),
                        parent_name: type_name.clone(),
                    });
                }

                let origin = match &ty.0 {
                    TypeKind::Named(_, o) | TypeKind::Generic(_, _, o) => {
                        o.as_ref().map(|s| s.as_ref())
                    }
                    _ => None,
                };
                let mut resolved_rb = None;
                if let Some(origin_mod) = origin {
                    resolved_rb = varn_checker::module_resolver::resolve_module_bind_ref(
                        origin_mod,
                    )
                    .or_else(|| {
                        varn_checker::module_resolver::resolve_stdlib_module_bind_ref(origin_mod)
                    });
                }

                if resolved_rb.is_none() {
                    for import_path in &self.import_paths {
                        let rb =
                            varn_checker::module_resolver::resolve_module_bind_ref(import_path)
                                .or_else(|| {
                                    varn_checker::module_resolver::resolve_stdlib_module_bind_ref(
                                        import_path,
                                    )
                                });
                        if let Some(ref_rb) = rb {
                            if ref_rb.get_class_entry(&type_name).is_some()
                                || ref_rb.get_interface_members_local(&type_name).is_some()
                                || ref_rb.get_enum_members_local(&type_name).is_some()
                                || ref_rb
                                    .type_members
                                    .namespaces
                                    .contains_key(type_name.as_str())
                            {
                                resolved_rb = Some(ref_rb);
                                break;
                            }
                        }
                    }
                }

                if let Some(rb) = resolved_rb {
                    let ms = if let Some(ms) = rb
                        .get_flattened_members(&type_name)
                        .or_else(|| rb.get_class_entry(&type_name).map(|e| &e.members))
                        .or_else(|| rb.get_interface_members_local(&type_name))
                    {
                        crate::pipeline::symbols::map_members(ms, &self.tokens)
                    } else if let Some(ms) = rb.get_enum_members_local(&type_name) {
                        crate::pipeline::symbols::map_enum_members(ms, &self.tokens)
                    } else if let Some(ms) = rb.type_members.namespaces.get(type_name.as_str()) {
                        crate::pipeline::symbols::map_members(ms, &self.tokens)
                    } else {
                        Vec::new()
                    };

                    if let Some(m) = self.find_member_recursive(&ms, name) {
                        if mapping.is_empty() {
                            return Some(ChainResult::DynamicMember {
                                member: m.clone(),
                                parent_name: type_name.clone(),
                            });
                        }
                        return Some(ChainResult::DynamicMember {
                            member: substitute_member_record(m, &mapping),
                            parent_name: type_name.clone(),
                        });
                    }
                }

                let builtins = varn_checker::core::loader::core_members_ref();
                if let Some(members_info) = builtins.class_members.get(type_name.as_str()) {
                    let ms =
                        crate::pipeline::symbols::map_members(&members_info.members, &self.tokens);
                    if let Some(m) = self.find_member_recursive(&ms, name) {
                        if mapping.is_empty() {
                            return Some(ChainResult::DynamicMember {
                                member: m.clone(),
                                parent_name: type_name.clone(),
                            });
                        }
                        return Some(ChainResult::DynamicMember {
                            member: substitute_member_record(m, &mapping),
                            parent_name: type_name.clone(),
                        });
                    }
                }
                None
            }
            TypeKind::Object(_) => {
                for sym in &self.symbols {
                    if &sym.ty == ty {
                        if let Some(m) = self.find_member_recursive(&sym.members, name) {
                            return Some(ChainResult::Member {
                                member: m,
                                parent_name: sym.name.clone(),
                            });
                        }
                    }
                    if let Some(m) = self.find_member_with_type_recursive(&sym.members, ty, name) {
                        let parent_name = self
                            .find_direct_parent_name_recursive(&sym.members, m)
                            .unwrap_or(&sym.name)
                            .to_owned();
                        return Some(ChainResult::Member {
                            member: m,
                            parent_name,
                        });
                    }
                }
                None
            }
            _ => None,
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    fn find_direct_parent_name_recursive<'a>(
        &'a self,
        members: &'a [MemberRecord],
        target: &MemberRecord,
    ) -> Option<&'a str> {
        for m in members {
            if m.members.iter().any(|inner| std::ptr::eq(inner, target)) {
                return Some(&m.name);
            }
            if let Some(name) = self.find_direct_parent_name_recursive(&m.members, target) {
                return Some(name);
            }
        }
        None
    }

    fn build_generic_mapping_lsp(
        &self,
        class_name: &str,
        type_args: &[Type],
    ) -> FxHashMap<Rc<str>, Type> {
        let sym = self.symbols.iter().find(|s| s.name == class_name);
        let type_params = sym.map(|s| s.type_params.as_slice()).unwrap_or(&[]);
        type_params
            .iter()
            .zip(type_args.iter())
            .map(|(k, v)| (Rc::from(k.as_str()), v.clone()))
            .collect()
    }

    pub(super) fn find_extension_member<'a>(
        &'a self,
        type_name: &str,
        member_name: &str,
    ) -> Option<&'a MemberRecord> {
        self.db
            .extension_members
            .get(type_name)
            .and_then(|members| members.iter().find(|m| m.name == member_name))
    }

    #[allow(clippy::only_used_in_recursion)]
    fn find_member_recursive<'a>(
        &'a self,
        members: &'a [MemberRecord],
        name: &str,
    ) -> Option<&'a MemberRecord> {
        for m in members {
            if m.name == name {
                return Some(m);
            }
            if let Some(nested) = self.find_member_recursive(&m.members, name) {
                return Some(nested);
            }
        }
        None
    }

    fn find_member_with_type_recursive<'a>(
        &'a self,
        members: &'a [MemberRecord],
        parent_ty: &Type,
        name: &str,
    ) -> Option<&'a MemberRecord> {
        for m in members {
            if &m.ty == parent_ty {
                if let Some(found) = self.find_member_recursive(&m.members, name) {
                    return Some(found);
                }
            }
            if let Some(found) = self.find_member_with_type_recursive(&m.members, parent_ty, name) {
                return Some(found);
            }
        }
        None
    }

    pub fn member_at_pos(
        &self,
        line: u32,
        col: u32,
    ) -> Option<(String, SymbolKind, &MemberRecord)> {
        let tok = self
            .tokens
            .iter()
            .enumerate()
            .find(|(idx, t)| {
                t.line == line
                    && is_identifier_like_for_hover(&self.tokens, *idx)
                    && t.col <= col
                    && col < t.col + t.length
            })
            .map(|(_, t)| t)?;

        for sym in self.symbols.iter().filter(|s| !s.is_from_stdlib) {
            if let Some(member) =
                self.find_member_at_pos_recursive(&sym.members, line, tok.col, &tok.lexeme)
            {
                let parent_name = self
                    .find_direct_parent_name_recursive(&sym.members, member)
                    .unwrap_or(&sym.name)
                    .to_owned();
                return Some((parent_name, sym.kind, member));
            }
        }

        for (type_name, members) in &self.db.extension_members {
            if let Some(member) =
                self.find_member_at_pos_recursive(members, line, tok.col, &tok.lexeme)
            {
                return Some((type_name.clone(), SymbolKind::Extension, member));
            }
        }
        None
    }

    #[allow(clippy::only_used_in_recursion)]
    fn find_member_at_pos_recursive<'a>(
        &'a self,
        members: &'a [MemberRecord],
        line: u32,
        col: u32,
        name: &str,
    ) -> Option<&'a MemberRecord> {
        for m in members {
            if m.line == line && m.name == name && col >= m.col && col < m.col + m.name.len() as u32
            {
                return Some(m);
            }
            if let Some(found) = self.find_member_at_pos_recursive(&m.members, line, col, name) {
                return Some(found);
            }
        }
        None
    }
}

fn substitute_member_record(m: &MemberRecord, mapping: &FxHashMap<Rc<str>, Type>) -> MemberRecord {
    let old_ty = m.ty.clone();
    let mut new_m = m.clone();
    new_m.ty = old_ty.map_generics(mapping);
    new_m.type_str = new_m.ty.to_string();

    if let TypeKind::Fn(ft) = &new_m.ty.0 {
        if !ft.is_arrow {
            new_m.type_str = ft.return_type.to_string();
        }
        new_m.params_str = format_params_lsp(ft);
    }

    if !m.members.is_empty() {
        new_m.members = m
            .members
            .iter()
            .map(|nm| substitute_member_record(nm, mapping))
            .collect();
    }

    new_m
}

fn format_params_lsp(ft: &varn_checker::types::FunctionType) -> String {
    ft.params
        .iter()
        .map(|p| {
            let rest = if p.is_rest { "..." } else { "" };
            let opt = if p.optional { "?" } else { "" };
            match &p.name {
                Some(n) => format!("{rest}{n}{opt}: {}", p.ty),
                None => format!("{rest}{}", p.ty),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn enrich_member_record(m: &mut MemberRecord) {
    m.type_str = if let TypeKind::Fn(ft) = &m.ty.0 {
        if !ft.is_arrow {
            ft.return_type.to_string()
        } else {
            m.ty.to_string()
        }
    } else {
        m.ty.to_string()
    };
    if let TypeKind::Fn(ft) = &m.ty.0 {
        m.params_str = format_params_lsp(ft);
        m.is_arrow = ft.is_arrow;
    }
}

fn type_name_str(ty: &Type) -> Option<String> {
    match &ty.0 {
        TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => Some(n.to_string()),
        TypeKind::Intrinsic(tag) => {
            if matches!(tag, TypeTag::Dynamic) {
                None
            } else {
                Some(tag.name().to_owned())
            }
        }
        TypeKind::Array(_) => Some("Array".to_owned()),
        TypeKind::Union(members) => {
            let non_null: Vec<_> = members
                .iter()
                .filter(|m| !matches!(m.0, TypeKind::Intrinsic(TypeTag::Null)))
                .collect();
            if non_null.len() == 1 {
                type_name_str(non_null[0])
            } else {
                None
            }
        }
        _ => None,
    }
}

fn literal_primitive_type(kind: TokenKind) -> Option<&'static str> {
    match kind {
        TokenKind::Str => Some(IntrinsicType::Str.as_str()),
        TokenKind::Char => Some(IntrinsicType::Char.as_str()),
        TokenKind::IntegerLiteral
        | TokenKind::HexLiteral
        | TokenKind::BinaryLiteral
        | TokenKind::OctalLiteral => Some(IntrinsicType::Int.as_str()),
        TokenKind::FloatLiteral => Some(IntrinsicType::Float.as_str()),
        TokenKind::DecimalLiteral => Some(IntrinsicType::Decimal.as_str()),
        TokenKind::BigIntLiteral => Some(IntrinsicType::BigInt.as_str()),
        TokenKind::True | TokenKind::False => Some(IntrinsicType::Bool.as_str()),
        _ => None,
    }
}
