use varn_checker::SymbolKind;
use varn_core::TokenKind;

use super::{ChainResult, DocumentState, MemberKind, MemberRecord, TokenRecord};

/// Turn the checker's verdict on a member access into the record the editor
/// features consume.
///
/// Everything here is read off [`varn_checker::MemberResolution`] — no signature
/// is reconstructed, so a member reports exactly what the stdlib declares.
fn member_record_from(res: &varn_checker::MemberResolution) -> MemberRecord {
    use varn_checker::ResolvedMemberKind as R;

    let (type_str, params_str) = match &res.member_ty.0 {
        varn_core::TypeKind::Fn(ft) => {
            let params = ft
                .params
                .iter()
                .map(|p| {
                    format!(
                        "{}: {}{}",
                        p.name.as_deref().unwrap_or("_"),
                        p.ty,
                        if p.optional { "?" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            (ft.return_type.to_string(), params)
        }
        _ => (res.member_ty.to_string(), String::new()),
    };

    let kind = match res.member_kind {
        R::EnumMember => MemberKind::EnumMember,
        R::Method | R::StaticMethod | R::ExtensionMethod => MemberKind::Method,
        R::Getter => MemberKind::Getter,
        R::Setter => MemberKind::Setter,
        R::Property | R::StaticProperty | R::ExtensionProperty => MemberKind::Property,
    };

    // `u32::MAX` is the "no source location" sentinel the features already test
    // for; a member from a precompiled interface blob has no range.
    let (line, col) = res
        .def_range
        .map(|r| (r.start.line.saturating_sub(1), r.start.column))
        .unwrap_or((u32::MAX, 0));

    MemberRecord {
        name: res.member_name.to_string(),
        type_str,
        params_str,
        is_static: matches!(res.member_kind, R::StaticMethod | R::StaticProperty),
        is_optional: false,
        kind,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        line,
        col,
        init_value: String::new(),
        ty: res.member_ty.clone(),
        symbol_id: None,
        members: Vec::new(),
    }
}

/// The name a type is known by, when it has one.
///
/// `None` for types that name no declaration — unions, tuples, function types,
/// `dynamic` — because the callers want a *declaration* to look members up on,
/// and there is none.
fn type_name_of(ty: &varn_checker::Type) -> Option<String> {
    use varn_core::TypeKind;
    match &ty.0 {
        TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => Some(n.to_string()),
        TypeKind::Intrinsic(tag) => {
            if *tag == varn_core::TypeTag::Dynamic {
                None
            } else {
                Some(tag.name().to_owned())
            }
        }
        TypeKind::Array(_) => Some(varn_core::IntrinsicType::Array.as_str().to_owned()),
        _ => None,
    }
}

impl DocumentState {
    pub fn offset_at_line_col(&self, line: u32, col: u32) -> u32 {
        let mut curr_line = 0;
        let mut curr_col = 0;
        let mut byte_offset = 0;
        for ch in self.source.chars() {
            if curr_line == line && curr_col == col {
                return byte_offset as u32;
            }
            if ch == '\n' {
                curr_line += 1;
                curr_col = 0;
            } else if ch != '\r' {
                curr_col += 1;
            }
            byte_offset += ch.len_utf8();
        }
        byte_offset as u32
    }

    pub fn resolve_chain_at(&self, line: u32, col: u32) -> Option<ChainResult<'_>> {
        let tok = self.identifier_token_at(line, col)?;

        let tok_idx_opt = self.tokens.iter().position(|t| t.offset == tok.offset);
        let after_dot = tok_idx_opt
            .map(|i| {
                i >= 1
                    && matches!(
                        self.tokens[i - 1].kind,
                        TokenKind::Dot | TokenKind::QuestionDot
                    )
            })
            .unwrap_or(false);

        // The checker resolved this member access: report what it decided.
        //
        // This used to consult a hand-written table of builtin signatures
        // (`Map.set`, `Range.toArray`, `Array.length`, …) *before* asking the
        // checker, so for every type it covered the table won — and the table
        // was a transcription of `std/` kept in sync by hand, matched by string
        // surgery on the receiver's printed type (`split('<')`, `ends_with("[]")`).
        // Any drift from the real stdlib surfaced as a confidently wrong hover.
        if after_dot {
            if let Some(res) = self.db.member_resolutions.get(&tok.offset) {
                return Some(ChainResult::DynamicMember {
                    member: member_record_from(res),
                    parent_name: type_name_of(&res.receiver_ty)
                        .unwrap_or_else(|| "dynamic".to_owned()),
                });
            }
        }

        if let Some(info) = self.db.expr_types.get(&tok.offset) {
            let is_fn = matches!(info.ty.0, varn_core::TypeKind::Fn(_));
            let (type_str, params_str) = match &info.ty.0 {
                varn_core::TypeKind::Fn(ft) => {
                    let mut params = Vec::new();
                    for p in &ft.params {
                        let name_str = p
                            .name
                            .as_ref()
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "_".to_string());
                        params.push(format!(
                            "{}: {}{}",
                            name_str,
                            p.ty,
                            if p.optional { "?" } else { "" }
                        ));
                    }
                    (ft.return_type.to_string(), params.join(", "))
                }
                _ => (info.ty.to_string(), String::new()),
            };

            if let Some(sid) = info.symbol_id {
                if sid < self.db.arena.len() {
                    let sym = self.db.arena.get(sid);

                    let sid_matches = sym.name.as_ref() == tok.lexeme.as_str();

                    let is_member_kind = sid_matches
                        && matches!(
                            sym.kind,
                            SymbolKind::Property | SymbolKind::Method | SymbolKind::EnumMember
                        );

                    if is_member_kind {
                        let parent_name = if after_dot {
                            self.resolve_receiver_type_name_at(tok)
                        } else {
                            String::new()
                        };

                        let name = tok.lexeme.as_str();
                        let by_id =
                            |m: &&MemberRecord| m.name.as_str() == name && m.symbol_id == Some(sid);
                        let hit = self
                            .symbols
                            .iter()
                            .find_map(|s| s.members.iter().find(by_id).map(|m| (s, m)));
                        if let Some((s, member)) = hit {
                            let clean_parent = if !parent_name.is_empty() && parent_name != "dynamic" {
                                parent_name
                            } else {
                                s.name.clone()
                            };
                            return Some(ChainResult::Member {
                                member,
                                parent_name: clean_parent,
                            });
                        }
                        let resolved_type_str = if !type_str.is_empty() && type_str != "dynamic" {
                            type_str
                        } else {
                            sym.ty
                                .as_ref()
                                .map(|t| t.to_string())
                                .unwrap_or_else(|| "dynamic".to_string())
                        };

                        return Some(ChainResult::DynamicMember {
                            member: MemberRecord {
                                name: sym.name.to_string(),
                                type_str: resolved_type_str,
                                params_str,
                                is_static: false,
                                is_optional: false,
                                kind: if is_fn || sym.kind == SymbolKind::Method {
                                    MemberKind::Method
                                } else if sym.kind == SymbolKind::EnumMember {
                                    MemberKind::EnumMember
                                } else {
                                    MemberKind::Property
                                },
                                is_arrow: false,
                                is_async: sym.is_async,
                                is_generator: sym.is_generator,
                                line: sym.line,
                                col: sym.col,
                                init_value: String::new(),
                                ty: info.ty.clone(),
                                symbol_id: Some(sid),
                                members: Vec::new(),
                            },
                            parent_name,
                        });
                    }

                    if after_dot {
                        let parent_name = self.resolve_receiver_type_name_at(tok);
                        return Some(ChainResult::DynamicMember {
                            member: MemberRecord {
                                name: tok.lexeme.clone(),
                                type_str,
                                params_str,
                                is_static: false,
                                is_optional: false,
                                kind: if is_fn {
                                    MemberKind::Method
                                } else {
                                    MemberKind::Property
                                },
                                is_arrow: false,
                                is_async: false,
                                is_generator: false,
                                line: tok.line,
                                col: tok.col,
                                init_value: String::new(),
                                ty: info.ty.clone(),
                                symbol_id: None,
                                members: Vec::new(),
                            },
                            parent_name,
                        });
                    }

                    if sid_matches {
                        let found = self
                            .symbols
                            .iter()
                            .find(|s| s.symbol_id == Some(sid))
                            .or_else(|| {
                                self.symbols.iter().find(|s| {
                                    !s.is_from_stdlib && s.line == sym.line && s.col == sym.col
                                })
                            });
                        if let Some(s) = found {
                            return Some(ChainResult::Symbol(s));
                        }
                    }
                }
            } else if after_dot {
                let parent_name = self.resolve_receiver_type_name_at(tok);

                if let Some(sid) = self.checker_symbol_id_at_token(tok) {
                    if let Some(s) = self.symbols.iter().find(|s| s.symbol_id == Some(sid)) {
                        return Some(ChainResult::Symbol(s));
                    }
                }
                return Some(ChainResult::DynamicMember {
                    member: MemberRecord {
                        name: tok.lexeme.clone(),
                        type_str,
                        params_str,
                        is_static: false,
                        is_optional: false,
                        kind: if is_fn {
                            MemberKind::Method
                        } else {
                            MemberKind::Property
                        },
                        is_arrow: false,
                        is_async: false,
                        is_generator: false,
                        line: tok.line,
                        col: tok.col,
                        init_value: String::new(),
                        ty: info.ty.clone(),
                        symbol_id: None,
                        members: Vec::new(),
                    },
                    parent_name,
                });
            }
        }

        if let Some(sid) = self.checker_symbol_id_at_token(tok) {
            if let Some(s) = self.symbols.iter().find(|s| s.symbol_id == Some(sid)) {
                return Some(ChainResult::Symbol(s));
            }
        }

        None
    }

    /// The name of the type `tok`'s member access is reading from.
    ///
    /// The checker records this per member access, typed, in
    /// `member_resolutions[offset].receiver_ty` — so that is where it comes
    /// from. This used to re-walk the entire AST from the root on every hover
    /// and goto (O(AST) per request) through a hand-written `match` that had to
    /// grow a case for each new `ExprKind`, and already had holes.
    ///
    /// The token fallback below is not a workaround for that: it answers the
    /// case the checker legitimately has nothing to say about, a member read off
    /// a `dynamic` value.
    pub fn resolve_receiver_type_name_at(&self, tok: &TokenRecord) -> String {
        if let Some(res) = self.db.member_resolutions.get(&tok.offset) {
            if let Some(name) = type_name_of(&res.receiver_ty) {
                return name;
            }
        }

        let tok_idx_opt = self.tokens.iter().position(|t| t.offset == tok.offset);
        let tok_idx = match tok_idx_opt {
            Some(i) if i >= 2 => i,
            _ => return "dynamic".to_string(),
        };

        let dot_tok = &self.tokens[tok_idx - 1];
        if dot_tok.kind != TokenKind::Dot && dot_tok.kind != TokenKind::QuestionDot {
            return "dynamic".to_string();
        }

        let prev_tok = &self.tokens[tok_idx - 2];
        if prev_tok.kind == TokenKind::Identifier || prev_tok.kind.can_be_identifier() {
            if let Some((sid, ty)) = self.db.resolve_at(&prev_tok.lexeme, prev_tok.offset) {
                if sid < self.db.arena.len() {
                    let sym = self.db.arena.get(sid);
                    if matches!(
                        sym.kind,
                        SymbolKind::Class
                            | SymbolKind::Interface
                            | SymbolKind::Enum
                            | SymbolKind::Struct
                            | SymbolKind::Namespace
                    ) {
                        return prev_tok.lexeme.clone();
                    }
                }
                let ty_str = ty.to_string();
                if !ty_str.is_empty() && ty_str != "unknown" && ty_str != "dynamic" {
                    return ty_str;
                }
            }
            return prev_tok.lexeme.clone();
        }

        "dynamic".to_string()
    }

    pub fn member_at_pos(
        &self,
        line: u32,
        col: u32,
    ) -> Option<(String, SymbolKind, &MemberRecord)> {
        let tok = self.identifier_token_at(line, col)?;

        if let Some(info) = self.db.expr_types.get(&tok.offset) {
            if let Some(sid) = info.symbol_id {
                let name = tok.lexeme.as_str();
                let member_pred = |m: &&MemberRecord| {
                    m.name.as_str() == name
                        && (m.symbol_id == Some(sid) || (m.line == tok.line && m.col == tok.col))
                };
                if let Some(s) = self
                    .symbols
                    .iter()
                    .find(|s| s.members.iter().any(|m| member_pred(&m)))
                {
                    let member = s.members.iter().find(member_pred).unwrap();
                    return Some((s.name.clone(), s.kind, member));
                }
            }
        }

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

        None
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
