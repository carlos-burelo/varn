use varn_checker::SymbolKind;
use varn_core::TokenKind;

use super::{ChainResult, DocumentState, TokenRecord};

/// The checker's verdict on a member access, as a summary.
///
/// A pass-through, not a translation: every field is read off
/// [`varn_checker::MemberResolution`]. This used to build a parallel
/// `MemberRecord` with the signature pre-flattened into `String`s.
fn summary_from_resolution(
    res: &varn_checker::MemberResolution,
) -> varn_checker::ResolvedMemberSummary {
    use varn_checker::ResolvedMemberKind as R;
    varn_checker::ResolvedMemberSummary {
        name: res.member_name.clone(),
        ty: res.member_ty.clone(),
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

/// A member as the checker's own type tables declare it.
///
/// Used at the declaration site, where no member *access* was resolved.
fn summary_from_class_member(
    m: &varn_checker::types::ClassMemberInfo,
) -> varn_checker::ResolvedMemberSummary {
    use varn_checker::{ClassMemberKind as C, NestedTypeKind as N, ResolvedMemberKind as R};
    varn_checker::ResolvedMemberSummary {
        name: m.name.clone(),
        ty: m.ty.clone(),
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

/// A summary for a member the checker typed but recorded no *access* for — a
/// member read off a `dynamic` value, or named at its own declaration.
fn summary_of(
    name: std::rc::Rc<str>,
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
                return Some(ChainResult::Member {
                    member: summary_from_resolution(res),
                    parent_name: type_name_of(&res.receiver_ty)
                        .unwrap_or_else(|| "dynamic".to_owned()),
                });
            }
        }

        if let Some(info) = self.db.expr_types.get(&tok.offset) {
            let is_fn = matches!(info.ty.0, varn_core::TypeKind::Fn(_));

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

                        if let Some(res) = self.db.member_resolutions.get(&tok.offset) {
                            let clean_parent = if !parent_name.is_empty()
                                && parent_name != "dynamic"
                            {
                                parent_name
                            } else {
                                type_name_of(&res.receiver_ty)
                                    .unwrap_or_else(|| "dynamic".to_owned())
                            };
                            return Some(ChainResult::Member {
                                member: summary_from_resolution(res),
                                parent_name: clean_parent,
                            });
                        }

                        return Some(ChainResult::Member {
                            member: summary_of(
                                sym.name.clone(),
                                info.ty.clone(),
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
                                std::rc::Rc::from(tok.lexeme.as_str()),
                                info.ty.clone(),
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
                        let found = self
                            .symbols()
                            .find(|s| s.id == sid)
                            .or_else(|| {
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
                        std::rc::Rc::from(tok.lexeme.as_str()),
                        info.ty.clone(),
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

    /// The member the cursor sits on, and the type it belongs to.
    ///
    /// `member_resolutions[offset]` is the checker's record of exactly this, so
    /// it is the answer. This used to scan every symbol's mirrored member tree,
    /// recursively, comparing names and line/column — an O(symbols x members)
    /// walk per request that could only find members the mirror happened to
    /// contain.
    pub fn member_at_pos(&self, line: u32, col: u32) -> Option<(String, varn_checker::ResolvedMemberSummary)> {
        let tok = self.identifier_token_at(line, col)?;

        // A member *access*: the checker recorded the receiver and the member.
        if let Some(res) = self.db.member_resolutions.get(&tok.offset) {
            let parent = type_name_of(&res.receiver_ty).unwrap_or_else(|| "dynamic".to_owned());
            return Some((parent, summary_from_resolution(res)));
        }

        // A member *declaration*. There is no access here, so nothing was
        // resolved — but the cursor still sits on a member, and callers that
        // key by member (references, rename) must get the same answer at the
        // declaration as at every use, or they silently match nothing.
        self.declared_member_at(tok)
    }

    /// The member a declaration-site token declares, and its owning type.
    ///
    /// Found by asking which type's member table claims this symbol, which is
    /// the checker's own record of ownership — not a mirror of it.
    fn declared_member_at(&self, tok: &TokenRecord) -> Option<(String, varn_checker::ResolvedMemberSummary)> {
        let sid = self.resolve_symbol_id_at_offset(tok.offset)?;
        let members = &self.db.bind.type_members;

        let owner = members
            .classes
            .iter()
            .find(|(_, e)| e.members.iter().any(|m| m.symbol_id == Some(sid)))
            .map(|(name, e)| (name.clone(), e.members.iter().find(|m| m.symbol_id == Some(sid))))
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
