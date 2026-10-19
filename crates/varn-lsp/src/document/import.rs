use rustc_hash::FxHashSet;
use varn_core::TokenKind;

use super::{token_lexeme, ImportPathContext, TokenRecord};

pub use varn_modules::resolver::{
    normalize_display_path, path_to_uri, percent_decode, relative_import_path, uri_to_path,
};

fn before(tok: &TokenRecord, line: u32, col: u32) -> bool {
    tok.line < line || (tok.line == line && tok.col < col)
}

pub fn import_path_at(
    source: &str,
    tokens: &[TokenRecord],
    line: u32,
    col: u32,
) -> Option<ImportPathContext> {
    let idx = tokens.iter().position(|t| {
        t.kind == TokenKind::Str && t.line == line && t.col < col && col <= t.col + t.length
    })?;
    let leads = idx
        .checked_sub(1)
        .is_some_and(|i| matches!(tokens[i].kind, TokenKind::From | TokenKind::Import));
    if !leads {
        return None;
    }
    let tok = &tokens[idx];
    let text = token_lexeme(source, tok);
    let quote = text.chars().next()?;
    let closed = text.len() >= 2 && text.ends_with(quote);
    if closed && col >= tok.col + tok.length {
        return None;
    }
    let content = if closed {
        &text[1..text.len() - 1]
    } else {
        &text[1..]
    };
    let typed = ((col - tok.col - 1) as usize).min(content.len());
    let prefix = &content[..typed];
    Some(ImportPathContext {
        prefix: prefix.to_owned(),
        specifier: if closed { content } else { prefix }.to_owned(),
        content_start_col: tok.col + 1,
    })
}

fn open_brace_of_import(tokens: &[TokenRecord], line: u32, col: u32) -> Option<usize> {
    let cursor = tokens.iter().rposition(|t| before(t, line, col))?;
    let mut i = cursor;
    loop {
        let kind = tokens[i].kind;
        if kind == TokenKind::RBrace || kind == TokenKind::Semicolon {
            return None;
        }
        if kind == TokenKind::LBrace {
            break;
        }
        i = i.checked_sub(1)?;
    }
    let opens_import = tokens[..i]
        .iter()
        .rev()
        .take_while(|t| t.kind != TokenKind::Semicolon && t.kind != TokenKind::RBrace)
        .any(|t| matches!(t.kind, TokenKind::Import | TokenKind::Export));
    opens_import.then_some(i)
}

pub fn named_import_module_at(
    source: &str,
    tokens: &[TokenRecord],
    line: u32,
    col: u32,
) -> Option<String> {
    let open = open_brace_of_import(tokens, line, col)?;
    let close = open
        + tokens[open..]
            .iter()
            .position(|t| t.kind == TokenKind::RBrace)?;
    let from = tokens
        .get(close + 1)
        .filter(|t| t.kind == TokenKind::From)?;
    let spec = tokens
        .get(close + 2)
        .filter(|t| t.kind == TokenKind::Str && t.line >= from.line)?;
    let text = token_lexeme(source, spec);
    let quote = text.chars().next()?;
    (text.len() >= 2 && text.ends_with(quote)).then(|| text[1..text.len() - 1].to_owned())
}

pub fn named_imported_names_at(
    source: &str,
    tokens: &[TokenRecord],
    line: u32,
    col: u32,
) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    let Some(open) = open_brace_of_import(tokens, line, col) else {
        return names;
    };
    let mut element_start = true;
    for tok in &tokens[open + 1..] {
        if tok.kind == TokenKind::RBrace {
            break;
        }
        if tok.kind == TokenKind::Comma {
            element_start = true;
            continue;
        }
        if element_start && tok.kind == TokenKind::Identifier {
            names.insert(token_lexeme(source, tok).to_owned());
        }
        element_start = false;
    }
    names
}
