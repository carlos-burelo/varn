use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_checker::{types::FunctionType, types::ObjectTypeMember, SymbolKind, Type};
use varn_core::{IntrinsicType, TokenKind, TypeKind, TypeTag};

use super::{ChainResult, DocumentState, MemberKind, MemberRecord};

fn is_function_type(ty: &Type) -> bool {
    match &ty.0 {
        TypeKind::Fn(_) => true,
        TypeKind::Intrinsic(TypeTag::Dynamic) => true,
        TypeKind::Union(members) => members.iter().any(is_function_type),
        _ => false,
    }
}

fn get_function_type(ty: &Type) -> Option<&FunctionType> {
    match &ty.0 {
        TypeKind::Fn(f) => Some(f),
        TypeKind::Union(members) => members.iter().find_map(get_function_type),
        _ => None,
    }
}

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
    pub fn resolve_chain_at(&self, line: u32, col: u32) -> Option<ChainResult> {
        let ident_idx = self.tokens.iter().enumerate().position(|(idx, t)| {
            t.line == line
                && is_identifier_like_for_hover(&self.tokens, idx)
                && t.col <= col
                && col < t.col + t.length
        })?;
        let target_tok = &self.tokens[ident_idx];
        if let Some(info) = self.db.expr_types.get(&target_tok.offset) {
            if let Some(sid) = info.symbol_id {
                if let Some(sym) = self.symbols.iter().find(|s| s.symbol_id == Some(sid)) {
                    return Some(ChainResult::Symbol(sym));
                }
            }
        }

        let mut chain: Vec<usize> = vec![ident_idx];
        let mut curr = ident_idx;

        let mut rparen_base_type: Option<Type> = None;

        while curr >= 2
            && matches!(
                self.tokens[curr - 1].kind,
                TokenKind::Dot | TokenKind::QuestionDot
            )
        {
            let prev = &self.tokens[curr - 2];

            if prev.kind == TokenKind::Identifier
                || prev.kind.can_be_identifier()
                || prev.kind.is_literal()
                || prev.kind == TokenKind::This
            {
                curr -= 2;
                chain.insert(0, curr);
            } else if prev.kind == TokenKind::RParen {
                rparen_base_type = self.db.expr_types.get(&prev.offset).map(|i| i.ty.clone());

                break;
            } else {
                break;
            }
        }

        let base_ident_idx = chain[0];
        let base_ident = &self.tokens[base_ident_idx];

        let mut current_type = if let Some(ty) = rparen_base_type.clone() {
            ty
        } else if let Some(info) = self.db.expr_types.get(&base_ident.offset) {
            let ty = info.ty.clone();

            if base_ident_idx == ident_idx && chain.len() == 1 {
                if let Some(sid) = info.symbol_id {
                    if let Some(sym) = self.symbols.iter().find(|s| s.symbol_id == Some(sid)) {
                        return Some(ChainResult::Symbol(sym));
                    }
                }
                if let Some(sym) = self.symbols.iter().find(|s| {
                    s.name == base_ident.lexeme
                        && matches!(
                            s.kind,
                            SymbolKind::Class
                                | SymbolKind::Interface
                                | SymbolKind::Namespace
                                | SymbolKind::Enum
                        )
                }) {
                    return Some(ChainResult::Symbol(sym));
                }
            }
            if ty.is_dynamic() {
                if let Some((_, resolved)) =
                    self.db.resolve_at(&base_ident.lexeme, base_ident.offset)
                {
                    resolved
                } else {
                    ty
                }
            } else {
                ty
            }
        } else if base_ident.kind == TokenKind::This {
            self.db
                .resolve_at("this", base_ident.offset)
                .map(|(_, ty)| ty)
                .unwrap_or(Type::Dynamic)
        } else if let Some(prim) = literal_primitive_type(base_ident.kind) {
            Type(TypeKind::Named(Rc::from(prim), None), false)
        } else {
            if let Some((sid, ty)) = self.db.resolve_at(&base_ident.lexeme, base_ident.offset) {
                if let Some(sym) = self.symbols.iter().find(|s| s.symbol_id == Some(sid)) {
                    return Some(ChainResult::Symbol(sym));
                }
                ty
            } else {
                Type::Dynamic
            }
        };

        let mut parent_name = type_name_str(&current_type).unwrap_or_else(|| {
            literal_primitive_type(base_ident.kind)
                .map(str::to_owned)
                .unwrap_or_else(|| base_ident.lexeme.clone())
        });

        if !current_type.is_dynamic() {
            let start_idx = if rparen_base_type.is_some() { 0 } else { 1 };
            for &idx in &chain[start_idx..] {
                let member_name = &self.tokens[idx].lexeme;

                let current_parent_name = type_name_str(&current_type);

                let base_type = if current_type.is_nullable() {
                    current_type.non_nullified()
                } else {
                    current_type.clone()
                };

                let found_member = match &base_type.0 {
                    TypeKind::Object(members) => members
                        .iter()
                        .find(|m| match m {
                            ObjectTypeMember::Property { name, .. } => name.as_ref() == member_name.as_str(),
                            ObjectTypeMember::Method { name, .. } => name.as_ref() == member_name.as_str(),
                            _ => false,
                        })
                        .map(|m| match m {
                            ObjectTypeMember::Property { ty, .. } => ty.clone(),
                            ObjectTypeMember::Method { return_type, .. } => (**return_type).clone(),
                            _ => Type::Dynamic,
                        }),
                    TypeKind::Named(_, _)
                    | TypeKind::Generic(_, _, _)
                    | TypeKind::Intrinsic(_) => {
                        let type_name = type_name_str(&base_type).unwrap();
                        let mapping = if let TypeKind::Generic(gname, type_args, _) = &base_type.0 {
                            self.build_generic_mapping_lsp(gname, type_args)
                        } else {
                            FxHashMap::default()
                        };
                        self.find_named_or_extension_member(&type_name, member_name)
                            .map(|m| {
                                if mapping.is_empty() {
                                    m.ty.clone()
                                } else {
                                    m.ty.map_generics(&mapping)
                                }
                            })
                    }
                    _ => None,
                };

                if let Some(ty) = found_member {
                    if let Some(n) = current_parent_name {
                        parent_name = n;
                    }

                    if idx == ident_idx {
                        if let Some(mut res) = self.find_chain_result(&base_type, member_name) {
                            if let Some(info) = self.db.expr_types.get(&self.tokens[idx].offset) {
                                if !info.ty.is_dynamic() {
                                    match &mut res {
                                        super::ChainResult::Member { member, .. } => {
                                            let mut new_member = (*member).clone();
                                            new_member.ty = info.ty.clone();
                                            enrich_member_record(&mut new_member);
                                            return Some(super::ChainResult::DynamicMember {
                                                member: new_member,
                                                parent_name: parent_name.clone(),
                                            });
                                        }
                                        super::ChainResult::DynamicMember { member, .. } => {
                                            member.ty = info.ty.clone();
                                            enrich_member_record(member);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            return Some(res);
                        }
                    }
                    current_type = match ty.0 {
                        TypeKind::Fn(ref ft) => (*ft.return_type).clone(),
                        _ => ty,
                    };
                } else {
                    if let Some(n) = current_parent_name {
                        parent_name = n;
                    }
                    break;
                }
            }
        }

        let target_ident = &self.tokens[ident_idx];
        if let Some(info) = self.db.expr_types.get(&target_ident.offset) {
            let ty = &info.ty;
            return Some(ChainResult::DynamicMember {
                member: MemberRecord {
                    name: target_ident.lexeme.clone(),
                    type_str: if let Some(ft) = get_function_type(ty) {
                        if !ft.is_arrow {
                            ft.return_type.to_string()
                        } else {
                            ty.to_string()
                        }
                    } else {
                        ty.to_string()
                    },
                    params_str: format_type_params_str(ty),
                    is_static: false,
                    is_optional: false,
                    kind: if is_function_type(ty) {
                        if chain.len() == 1 {
                            MemberKind::Function
                        } else {
                            MemberKind::Method
                        }
                    } else {
                        MemberKind::Property
                    },
                    is_arrow: if let Some(ft) = get_function_type(ty) {
                        ft.is_arrow
                    } else {
                        false
                    },
                    is_async: false,
                    is_generator: false,
                    line: target_ident.line,
                    col: target_ident.col,
                    init_value: String::new(),
                    ty: ty.clone(),
                    symbol_id: info.symbol_id,
                    members: Vec::new(),
                },
                parent_name: parent_name.clone(),
            });
        }

        None
    }

    fn find_chain_result(&self, ty: &Type, name: &str) -> Option<ChainResult> {
        let type_name = type_name_str(ty)?;
        match &ty.0 {
            TypeKind::Named(_, _)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Intrinsic(_) => {
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
                if let Some(sym) = self.symbols.iter().find(|s| s.name == name) {
                    return Some(ChainResult::Symbol(sym));
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

    pub(super) fn find_named_or_extension_member<'a>(
        &'a self,
        type_name: &str,
        member_name: &str,
    ) -> Option<&'a MemberRecord> {
        self.symbols
            .iter()
            .find(|s| s.name == type_name)
            .and_then(|sym| sym.members.iter().find(|m| m.name == member_name))
            .or_else(|| self.find_extension_member(type_name, member_name))
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

        for sym in &self.symbols {
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
            if m.line == line && m.name == name && m.col <= col {
                return Some(m);
            }
            if let Some(found) = self.find_member_at_pos_recursive(&m.members, line, col, name) {
                return Some(found);
            }
        }
        None
    }
}

fn format_type_params_str(ty: &Type) -> String {
    if let Some(ft) = get_function_type(ty) {
        let mut out = String::new();
        for (i, p) in ft.params.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            if let Some(name) = &p.name {
                out.push_str(name);
            } else {
                out.push_str(&format!("arg{i}"));
            }
            if p.optional {
                out.push('?');
            }
            out.push_str(": ");
            out.push_str(&format!("{}", p.ty));
        }
        out
    } else {
        String::new()
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
        TypeKind::Intrinsic(tag) => Some(tag.name().to_owned()),
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
