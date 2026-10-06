use varn_checker::SymbolKind;
use varn_core::TokenKind;

use super::{ChainResult, DocumentState, TokenRecord};






fn summary_from_resolution(
    res: &varn_checker::MemberResolution,
) -> varn_checker::ResolvedMemberSummary {
    use varn_checker::ResolvedMemberKind as R;
    varn_checker::ResolvedMemberSummary {
        name: res.member_name.clone(),
        ty: res.member_ty,
        kind: res.member_kind,
        is_static: matches!(res.member_kind, R::StaticMethod | R::StaticProperty),
        optional: false,
        readonly: false,
        def_line: res.def_range.map(|r| r.start.line),
        def_col: res.def_range.map(|r| r.start.column).unwrap_or(0),
        is_async: false,
        is_generator: false,
    }
}




fn summary_from_class_member(
    m: &varn_checker::types::ClassMemberInfo,
) -> varn_checker::ResolvedMemberSummary {
    use varn_checker::{ClassMemberKind as C, NestedTypeKind as N, ResolvedMemberKind as R};
    varn_checker::ResolvedMemberSummary {
        name: m.name.clone(),
        ty: m.ty,
        kind: match m.kind {
            C::Method | C::Function => R::Method,
            C::Constructor => R::Constructor,
            C::Getter => R::Getter,
            C::Setter => R::Setter,
            C::Class => R::NestedType(N::Class),
            C::Interface => R::NestedType(N::Interface),
            C::Namespace => R::NestedType(N::Namespace),
            C::Enum => R::NestedType(N::Enum),
            C::Struct => R::NestedType(N::Struct),
            C::Variable | C::Property => R::Property,
        },
        is_static: m.is_static,
        optional: m.is_optional,
        readonly: m.is_readonly,
        def_line: (m.line > 0).then_some(m.line),
        def_col: m.col,
        is_async: m.is_async,
        is_generator: m.is_generator,
    }
}



fn summary_of(
    name: std::sync::Arc<str>,
    ty: varn_checker::Type,
    kind: varn_checker::ResolvedMemberKind,
    line: u32,
    col: u32,
) -> varn_checker::ResolvedMemberSummary {
    varn_checker::ResolvedMemberSummary {
        name,
        ty,
        kind,
        is_static: false,
        optional: false,
        readonly: false,
        def_line: (line > 0).then_some(line),
        def_col: col,
        is_async: false,
        is_generator: false,
    }
}


const DYNAMIC: &str = varn_core::LangPrimitive::Dynamic.name();

impl DocumentState {
    
    
    
    
    
    
    pub fn offset_at_line_col(&self, line: u32, col: u32) -> u32 {
        crate::document::position::byte_offset(
            &self.source,
            tower_lsp::lsp_types::Position::new(line, col),
        ) as u32
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

        
        
        
        
        
        
        
        
        if after_dot {
            if let Some(res) = self.db.member_resolutions.get(&tok.offset) {
                return Some(ChainResult::Member {
                    member: summary_from_resolution(res),
                    parent_name: self
                        .db
                        .decl_name(&res.receiver_ty)
                        .unwrap_or_else(|| DYNAMIC.to_string()),
                });
            }
        }

        if let Some(entry) = self.expr_entry_at_offset(tok.offset) {
            let is_fn = matches!(self.db.ty_kind(&entry.ty), varn_core::TypeKind::Fn(_));

            if let Some(sid) = entry.symbol_id {
                if sid < self.db.bind.arena.len() {
                    let sym = self.db.bind.arena.get(sid);

                    let sid_matches = self.name(sym.name) == self.lexeme(tok);

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

                        if let Some(res) = self.db.member_resolutions.get(&tok.offset) {
                            let clean_parent = if !parent_name.is_empty() && parent_name != DYNAMIC
                            {
                                parent_name
                            } else {
                                self.db
                                    .decl_name(&res.receiver_ty)
                                    .unwrap_or_else(|| DYNAMIC.to_string())
                            };
                            return Some(ChainResult::Member {
                                member: summary_from_resolution(res),
                                parent_name: clean_parent,
                            });
                        }

                        return Some(ChainResult::Member {
                            member: summary_of(
                                std::sync::Arc::from(self.name(sym.name)),
                                entry.ty,
                                if is_fn || sym.kind == SymbolKind::Method {
                                    varn_checker::ResolvedMemberKind::Method
                                } else if sym.kind == SymbolKind::EnumMember {
                                    varn_checker::ResolvedMemberKind::EnumMember
                                } else {
                                    varn_checker::ResolvedMemberKind::Property
                                },
                                sym.line,
                                sym.col,
                            ),
                            parent_name,
                        });
                    }

                    if after_dot {
                        let parent_name = self.resolve_receiver_type_name_at(tok);
                        return Some(ChainResult::Member {
                            member: summary_of(
                                std::sync::Arc::from(self.lexeme(tok)),
                                entry.ty,
                                if is_fn {
                                    varn_checker::ResolvedMemberKind::Method
                                } else {
                                    varn_checker::ResolvedMemberKind::Property
                                },
                                tok.line,
                                tok.col,
                            ),
                            parent_name,
                        });
                    }

                    if sid_matches {
                        let found = self.symbols().find(|s| s.id == sid).or_else(|| {
                            self.symbols().find(|s| {
                                !s.is_from_stdlib() && s.line() == sym.line && s.col() == sym.col
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
                    if let Some(s) = self.symbols().find(|s| s.id == sid) {
                        return Some(ChainResult::Symbol(s));
                    }
                }
                return Some(ChainResult::Member {
                    member: summary_of(
                        std::sync::Arc::from(self.lexeme(tok)),
                        entry.ty,
                        if is_fn {
                            varn_checker::ResolvedMemberKind::Method
                        } else {
                            varn_checker::ResolvedMemberKind::Property
                        },
                        tok.line,
                        tok.col,
                    ),
                    parent_name,
                });
            }
        }

        if let Some(sid) = self.checker_symbol_id_at_token(tok) {
            if let Some(s) = self.symbols().find(|s| s.id == sid) {
                return Some(ChainResult::Symbol(s));
            }
        }

        None
    }

    
    pub fn resolve_receiver_type_name_at(&self, tok: &TokenRecord) -> String {
        if let Some(res) = self.db.member_resolutions.get(&tok.offset) {
            if let Some(name) = self.db.decl_name(&res.receiver_ty) {
                return name;
            }
        }

        let tok_idx_opt = self.tokens.iter().position(|t| t.offset == tok.offset);
        let tok_idx = match tok_idx_opt {
            Some(i) if i >= 2 => i,
            _ => return DYNAMIC.to_string(),
        };

        let dot_tok = &self.tokens[tok_idx - 1];
        if dot_tok.kind != TokenKind::Dot && dot_tok.kind != TokenKind::QuestionDot {
            return DYNAMIC.to_string();
        }

        let prev_tok = &self.tokens[tok_idx - 2];
        if prev_tok.kind == TokenKind::Identifier || prev_tok.kind.can_be_identifier() {
            if let Some((sid, ty)) = self.db.resolve_at(self.lexeme(prev_tok), prev_tok.offset) {
                if sid < self.db.bind.arena.len() {
                    let sym = self.db.bind.arena.get(sid);
                    if matches!(
                        sym.kind,
                        SymbolKind::Class
                            | SymbolKind::Interface
                            | SymbolKind::Enum
                            | SymbolKind::Struct
                            | SymbolKind::Namespace
                    ) {
                        return self.lexeme(prev_tok).to_owned();
                    }
                }
                if let Some(name) = self.db.decl_name(&ty) {
                    return name;
                }
            }
            return self.lexeme(prev_tok).to_owned();
        }

        DYNAMIC.to_string()
    }

    
    pub fn member_at_pos(
        &self,
        line: u32,
        col: u32,
    ) -> Option<(String, varn_checker::ResolvedMemberSummary)> {
        let tok = self.identifier_token_at(line, col)?;

        
        if let Some(res) = self.db.member_resolutions.get(&tok.offset) {
            let parent = self
                .db
                .decl_name(&res.receiver_ty)
                .unwrap_or_else(|| DYNAMIC.to_string());
            return Some((parent, summary_from_resolution(res)));
        }

        
        
        
        
        self.declared_member_at(tok)
    }

    
    
    
    
    fn declared_member_at(
        &self,
        tok: &TokenRecord,
    ) -> Option<(String, varn_checker::ResolvedMemberSummary)> {
        let sid = self.resolve_symbol_id_at_offset(tok.offset)?;
        let members = &self.db.bind.type_members;

        let owner = members
            .classes
            .iter()
            .find(|(_, e)| e.members.iter().any(|m| m.symbol_id == Some(sid)))
            .map(|(name, e)| {
                (
                    name.clone(),
                    e.members.iter().find(|m| m.symbol_id == Some(sid)),
                )
            })
            .or_else(|| {
                members.interfaces.iter().find_map(|(name, ms)| {
                    ms.iter()
                        .any(|m| m.symbol_id == Some(sid))
                        .then(|| (name.clone(), ms.iter().find(|m| m.symbol_id == Some(sid))))
                })
            })
            .or_else(|| {
                members.namespaces.iter().find_map(|(name, ms)| {
                    ms.iter()
                        .any(|m| m.symbol_id == Some(sid))
                        .then(|| (name.clone(), ms.iter().find(|m| m.symbol_id == Some(sid))))
                })
            })?;

        let (type_name, info) = (owner.0, owner.1?);
        Some((type_name.to_string(), summary_from_class_member(info)))
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
}
