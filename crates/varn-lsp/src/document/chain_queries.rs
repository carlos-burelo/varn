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

    pub fn resolve_chain_at(&self, line: u32, col: u32) -> Option<ChainResult> {
        let tok = self.tokens.iter().find(|t| {
            t.line == line
                && (t.kind == TokenKind::Identifier || t.kind.can_be_identifier())
                && t.col <= col
                && col < t.col + t.length
        })?;

        // 1. Look up the token's offset in expr_types
        if let Some(info) = self.db.expr_types.get(&tok.offset) {
            if let Some(sid) = info.symbol_id {
                if sid < self.db.arena.len() {
                    let sym = self.db.arena.get(sid);
                    
                    if sym.kind == SymbolKind::Property || sym.kind == SymbolKind::Method || sym.kind == SymbolKind::EnumMember {
                        // Find the class or interface that contains this member
                        if let Some(s) = self.symbols.iter().find(|s| {
                            s.members.iter().any(|m| m.symbol_id == Some(sid) || (m.line == sym.line && m.col == sym.col))
                        }) {
                            let member = s.members.iter().find(|m| m.symbol_id == Some(sid) || (m.line == sym.line && m.col == sym.col)).unwrap();
                            return Some(ChainResult::Member {
                                member,
                                parent_name: s.name.clone(),
                            });
                        }

                        // Handle members of imported or standard library types
                        let parent_name = sym.origin_module.as_ref()
                            .map(|o| o.rsplit(['/', '\\']).next().unwrap_or(o).replace(".vn", ""))
                            .unwrap_or_else(|| "dynamic".to_string());

                        return Some(ChainResult::DynamicMember {
                            member: MemberRecord {
                                name: sym.name.to_string(),
                                type_str: sym.ty.as_ref().map(|t| t.to_string()).unwrap_or_else(|| "dynamic".to_string()),
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

                    // If it is a top-level symbol (not a member)
                    if let Some(s) = self.symbols.iter().find(|s| s.symbol_id == Some(sid) || (s.line == sym.line && s.col == sym.col)) {
                        return Some(ChainResult::Symbol(s));
                    }
                }
            }
        }

        // 2. Fall back to resolve_at using scope resolution if no type-checking info is present
        if let Some((sid, _)) = self.db.resolve_at(&tok.lexeme, tok.offset) {
            if let Some(s) = self.symbols.iter().find(|s| s.symbol_id == Some(sid)) {
                return Some(ChainResult::Symbol(s));
            }
        }

        // 3. Fall back to simple name matching in local symbols
        if let Some(s) = self.symbols.iter().find(|s| s.name == tok.lexeme) {
            return Some(ChainResult::Symbol(s));
        }

        None
    }

    pub fn member_at_pos(
        &self,
        line: u32,
        col: u32,
    ) -> Option<(String, SymbolKind, &MemberRecord)> {
        let tok = self.tokens.iter().find(|t| {
            t.line == line
                && (t.kind == TokenKind::Identifier || t.kind.can_be_identifier())
                && t.col <= col
                && col < t.col + t.length
        })?;

        // 1. Look up token in expr_types to see if it's a known member
        if let Some(info) = self.db.expr_types.get(&tok.offset) {
            if let Some(sid) = info.symbol_id {
                if let Some(s) = self.symbols.iter().find(|s| {
                    s.members.iter().any(|m| m.symbol_id == Some(sid) || (m.line == tok.line && m.col == tok.col))
                }) {
                    let member = s.members.iter().find(|m| m.symbol_id == Some(sid) || (m.line == tok.line && m.col == tok.col)).unwrap();
                    return Some((s.name.clone(), s.kind, member));
                }
            }
        }

        // 2. Fallback to declaration matching in local symbols
        for sym in self.symbols.iter().filter(|s| !s.is_from_stdlib) {
            if let Some(member) = self.find_member_at_pos_recursive(&sym.members, line, tok.col, &tok.lexeme) {
                let parent_name = self.find_direct_parent_name_recursive(&sym.members, member)
                    .unwrap_or(&sym.name)
                    .to_owned();
                return Some((parent_name, sym.kind, member));
            }
        }

        None
    }
    pub(crate) fn find_extension_member<'a>(&'a self, type_name: &str, member_name: &str) -> Option<&'a MemberRecord> {
        self.db.extension_members.get(type_name)
            .and_then(|members| members.iter().find(|m| m.name == member_name))
    }

    pub(crate) fn expr_info_at_token(&self, tok: &super::TokenRecord) -> Option<&varn_checker::ExprInfo> {
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
            if m.line == line && m.name == name && col >= m.col && col < m.col + m.name.len() as u32 {
                return Some(m);
            }
            if let Some(found) = self.find_member_at_pos_recursive(&m.members, line, col, name) {
                return Some(found);
            }
        }
        None
    }
}
