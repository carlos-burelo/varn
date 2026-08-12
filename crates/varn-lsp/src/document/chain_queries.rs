use varn_checker::SymbolKind;
use varn_core::TokenKind;

use super::{ChainResult, DocumentState, MemberKind, MemberRecord};

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

    pub fn resolve_chain_at(&self, line: u32, col: u32) -> Option<ChainResult<'_>> {
        let tok = self.identifier_token_at(line, col)?;

        if let Some(info) = self.db.expr_types.get(&tok.offset) {
            if let Some(sid) = info.symbol_id {
                if sid < self.db.arena.len() {
                    let sym = self.db.arena.get(sid);

                    let sid_matches = sym.name.as_ref() == tok.lexeme.as_str();

                    let is_member_kind = sid_matches
                        && matches!(
                            sym.kind,
                            SymbolKind::Property | SymbolKind::Method | SymbolKind::EnumMember
                        );

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

                    if is_member_kind {
                        let name = tok.lexeme.as_str();
                        // SymbolId is authoritative; the (line, col) fallback is
                        // only for members the binder left without an id, and is
                        // kept after it so a cross-file positional coincidence
                        // cannot win over the real member.
                        let by_id =
                            |m: &&MemberRecord| m.name.as_str() == name && m.symbol_id == Some(sid);
                        let by_pos = |m: &&MemberRecord| {
                            m.name.as_str() == name && m.line == sym.line && m.col == sym.col
                        };
                        let hit = self
                            .symbols
                            .iter()
                            .find_map(|s| s.members.iter().find(by_id).map(|m| (s, m)))
                            .or_else(|| {
                                self.symbols
                                    .iter()
                                    .find_map(|s| s.members.iter().find(by_pos).map(|m| (s, m)))
                            });
                        if let Some((s, member)) = hit {
                            return Some(ChainResult::Member {
                                member,
                                parent_name: s.name.clone(),
                            });
                        }

                        let parent_name = sym
                            .origin_module
                            .as_ref()
                            .map(|o| o.rsplit(['/', '\\']).next().unwrap_or(o).replace(".vn", ""))
                            .unwrap_or_else(|| "dynamic".to_string());

                        return Some(ChainResult::DynamicMember {
                            member: MemberRecord {
                                name: sym.name.to_string(),
                                type_str: sym
                                    .ty
                                    .as_ref()
                                    .map(|t| t.to_string())
                                    .unwrap_or_else(|| "dynamic".to_string()),
                                params_str: String::new(),
                                is_static: false,
                                is_optional: false,
                                kind: match sym.kind {
                                    SymbolKind::Property => MemberKind::Property,
                                    SymbolKind::Method => MemberKind::Method,
                                    SymbolKind::EnumMember => MemberKind::EnumMember,
                                    _ => MemberKind::Property,
                                },
                                is_arrow: false,
                                is_async: sym.is_async,
                                is_generator: sym.is_generator,
                                line: sym.line,
                                col: sym.col,
                                init_value: String::new(),
                                ty: sym.ty.clone().unwrap_or(varn_checker::Type::Dynamic),
                                symbol_id: Some(sid),
                                members: Vec::new(),
                            },
                            parent_name,
                        });
                    }

                    if after_dot {
                        if let Some(tok_idx) = tok_idx_opt {
                            let parent_name = if tok_idx >= 2 {
                                self.tokens[tok_idx - 2].lexeme.clone()
                            } else {
                                "dynamic".to_string()
                            };
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

                    if sid_matches {
                        // SymbolId is the authoritative key. The (line, col)
                        // fallback exists only for SymbolRecords that never got a
                        // symbol_id, and must stay file-local: stdlib symbols carry
                        // line/col from *their own* source file, so a positional
                        // match against them is a cross-file coordinate collision
                        // (e.g. a user param at l4c24 vs stdlib `input` at l4c24).
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
            } else {
                if let Some(tok_idx) = self.tokens.iter().position(|t| t.offset == tok.offset) {
                    let parent_name = if tok_idx >= 2
                        && (self.tokens[tok_idx - 1].kind == TokenKind::Dot
                            || self.tokens[tok_idx - 1].kind == TokenKind::QuestionDot)
                    {
                        self.tokens[tok_idx - 2].lexeme.clone()
                    } else {
                        "dynamic".to_string()
                    };

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
                                MemberKind::Function
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
        }

        if let Some(sid) = self.checker_symbol_id_at_token(tok) {
            if let Some(s) = self.symbols.iter().find(|s| s.symbol_id == Some(sid)) {
                return Some(ChainResult::Symbol(s));
            }
        }

        None
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
