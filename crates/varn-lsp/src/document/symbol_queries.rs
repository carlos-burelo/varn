use varn_checker::SymbolKind;
use varn_core::TokenKind;

use super::{DocumentState, MethodHoverInfo, SymbolRecord};

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
    pub fn symbol_at_line(&self, line: u32) -> Option<&SymbolRecord> {
        self.symbols.iter().find(|s| s.line == line)
    }

    pub fn symbol_at_pos(&self, line: u32, col: u32) -> Option<&SymbolRecord> {
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

        if self.member_at_pos(line, col).is_some() {
            return None;
        }

        self.checker_symbol_at(line, col)
            .filter(|sym| sym.name == tok.lexeme)
    }

    pub fn symbols_named(&self, name: &str) -> Vec<&SymbolRecord> {
        self.symbols.iter().filter(|s| s.name == name).collect()
    }

    pub fn param_decl_at_pos(&self, line: u32, col: u32) -> Option<(String, String)> {
        let ident_idx = self.tokens.iter().position(|t| {
            t.line == line
                && t.kind == TokenKind::Identifier
                && t.col <= col
                && col < t.col + t.length
        })?;
        let prev = ident_idx.checked_sub(1).and_then(|j| self.tokens.get(j))?;
        if !matches!(prev.kind, TokenKind::LParen | TokenKind::Comma) {
            return None;
        }
        let next = self.tokens.get(ident_idx + 1)?;
        if next.kind != TokenKind::Colon {
            return None;
        }

        let param_name = self.tokens[ident_idx].lexeme.clone();
        if !is_likely_param_list(&self.tokens, ident_idx) {
            return None;
        }
        let mut type_lexemes: Vec<(u32, u32, &str)> = Vec::new();
        let mut depth = 0i32;
        let mut j = ident_idx + 2;
        while let Some(t) = self.tokens.get(j) {
            match t.kind {
                TokenKind::LParen | TokenKind::LAngle => depth += 1,
                TokenKind::RParen if depth == 0 => break,
                TokenKind::RParen => depth -= 1,
                TokenKind::RAngle if depth > 0 => depth -= 1,
                TokenKind::Comma if depth == 0 => break,
                TokenKind::Eq if depth == 0 => break,
                _ => {}
            }
            type_lexemes.push((t.line, t.col, &t.lexeme));
            j += 1;
        }

        let type_str = reconstruct_spaced_tokens(&type_lexemes);
        Some((param_name, type_str))
    }

    pub fn param_usage_at_pos(&self, line: u32, col: u32) -> Option<(String, String)> {
        let ident_tok = self.tokens.iter().find(|t| {
            t.line == line
                && t.kind == TokenKind::Identifier
                && t.col <= col
                && col < t.col + t.length
        })?;

        let ident_idx = self
            .tokens
            .iter()
            .position(|t| std::ptr::eq(t, ident_tok))?;
        if ident_idx > 0 && self.tokens[ident_idx - 1].kind == TokenKind::Dot {
            return None;
        }
        if self.tokens.get(ident_idx + 1).map(|t| t.kind) == Some(TokenKind::LParen) {
            return None;
        }

        let prev_kind = ident_idx.checked_sub(1).map(|j| self.tokens[j].kind);
        let next_kind = self.tokens.get(ident_idx + 1).map(|t| t.kind);
        if matches!(prev_kind, Some(TokenKind::LParen) | Some(TokenKind::Comma))
            && next_kind == Some(TokenKind::Colon)
        {
            return None;
        }

        let (sym_id, ty) = self.db.resolve_at(&ident_tok.lexeme, ident_tok.offset)?;
        let sym = self.db.arena.get(sym_id);
        if sym.kind != varn_checker::SymbolKind::Parameter {
            return None;
        }
        Some((ident_tok.lexeme.clone(), ty.to_string()))
    }

    pub fn type_param_at_pos(&self, line: u32, col: u32) -> Option<String> {
        let tok = self.tokens.iter().find(|t| {
            t.line == line
                && t.kind == TokenKind::Identifier
                && t.col <= col
                && col < t.col + t.length
        })?;
        if self.type_param_names.contains(tok.lexeme.as_str()) {
            Some(tok.lexeme.clone())
        } else {
            None
        }
    }

    pub fn method_at_pos(&self, line: u32, col: u32) -> Option<MethodHoverInfo> {
        let ident_idx = self.tokens.iter().enumerate().position(|(idx, t)| {
            t.line == line
                && is_identifier_like_for_hover(&self.tokens, idx)
                && t.col <= col
                && col < t.col + t.length
        })?;
        let ident_tok = &self.tokens[ident_idx];
        let method_name = &ident_tok.lexeme;

        if ident_idx < 2 {
            return None;
        }
        let dot_tok = &self.tokens[ident_idx - 1];
        if dot_tok.kind != TokenKind::Dot {
            return None;
        }
        let receiver_tok = &self.tokens[ident_idx - 2];

        if receiver_tok.kind == TokenKind::This {
            let enclosing = self
                .symbols
                .iter()
                .filter(|s| {
                    matches!(s.kind, SymbolKind::Class | SymbolKind::Interface) && s.line <= line
                })
                .max_by_key(|s| s.line)?;

            let member = enclosing.members.iter().find(|m| m.name == *method_name)?;
            return Some(MethodHoverInfo {
                receiver: "this".to_owned(),
                class_name: enclosing.name.clone(),
                method_name: method_name.clone(),
                return_type: member.type_str.clone(),
                params_str: member.params_str.clone(),
                is_static: false,
                parent_kind: enclosing.kind,
                init_value: member.init_value.clone(),
            });
        }

        if receiver_tok.kind != TokenKind::Identifier {
            return None;
        }
        let receiver_name = &receiver_tok.lexeme;

        let receiver_ty = self
            .db
            .expr_types
            .get(&receiver_tok.offset)
            .map(|i| i.ty.clone())
            .or_else(|| {
                self.db
                    .resolve_at(&receiver_tok.lexeme, receiver_tok.offset)
                    .map(|(_, ty)| ty)
            });

        let class_name = match receiver_ty.as_ref().map(|t| &t.0) {
            Some(varn_core::TypeKind::Named(n, _)) => n.to_string(),
            Some(varn_core::TypeKind::Generic(n, _, _)) => n.to_string(),
            _ => receiver_name.clone(),
        };

        let receiver_sym = self.symbols.iter().find(|s| s.name == *receiver_name)?;
        let class_sym: &SymbolRecord = if !receiver_sym.members.is_empty()
            && matches!(
                receiver_sym.kind,
                SymbolKind::Var | SymbolKind::Let | SymbolKind::Const | SymbolKind::Enum
            ) {
            receiver_sym
        } else {
            self.symbols
                .iter()
                .find(|s| s.name == class_name.as_str())?
        };
        let member = class_sym
            .members
            .iter()
            .find(|m| m.name == *method_name)
            .or_else(|| self.find_extension_member(&class_sym.name, method_name))?;

        Some(MethodHoverInfo {
            receiver: receiver_name.clone(),
            class_name: class_sym.name.clone(),
            method_name: method_name.clone(),
            return_type: member.type_str.clone(),
            params_str: member.params_str.clone(),
            is_static: member.is_static,
            parent_kind: class_sym.kind,
            init_value: member.init_value.clone(),
        })
    }
}

fn is_likely_param_list(tokens: &[super::TokenRecord], ident_idx: usize) -> bool {
    let mut paren_depth = 0i32;
    let mut brace_depth = 0i32;
    let mut bracket_depth = 0i32;

    for j in (0..ident_idx).rev() {
        match tokens[j].kind {
            TokenKind::RParen => paren_depth += 1,
            TokenKind::LParen => {
                if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
                    return true;
                }
                paren_depth -= 1;
            }
            TokenKind::RBrace => brace_depth += 1,
            TokenKind::LBrace => {
                if brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 {
                    return false;
                }
                brace_depth -= 1;
            }
            TokenKind::RBracket => bracket_depth += 1,
            TokenKind::LBracket => {
                if bracket_depth == 0 && paren_depth == 0 && brace_depth == 0 {
                    return false;
                }
                bracket_depth -= 1;
            }
            _ => {}
        }
    }
    false
}

fn reconstruct_spaced_tokens(parts: &[(u32, u32, &str)]) -> String {
    if parts.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut prev_line = parts[0].0;
    let mut prev_end = parts[0].1;
    for (ln, col, lex) in parts {
        if !out.is_empty() && (*ln > prev_line || *col > prev_end) {
            out.push(' ');
        }
        out.push_str(lex);
        prev_line = *ln;
        prev_end = col + lex.len() as u32;
    }
    out
}
