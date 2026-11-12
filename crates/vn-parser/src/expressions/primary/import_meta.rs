use crate::error::{PResult, ParseError};
use crate::stream::TokenStream;
use vn_core::ast::{ExprId, ExprKind, MetaProperty};
use vn_core::{ContextualKeyword, TokenKind};

pub(super) fn parse_import_meta(s: &mut TokenStream) -> PResult<ExprId> {
    let start = s.range();
    s.expect(TokenKind::Import)?;
    s.expect(TokenKind::Dot)?;
    let meta = s.expect_id()?;
    if s.interner.resolve(meta) != ContextualKeyword::Meta.as_str() {
        return Err(ParseError::invalid(format!(
            "expected `import.{}`",
            ContextualKeyword::Meta.as_str()
        )));
    }
    s.expect(TokenKind::Dot)?;
    let name = s.expect_id()?;
    let text = s.interner.resolve(name);
    let property = MetaProperty::of(text).ok_or_else(|| {
        ParseError::invalid(format!(
            "unknown import.{} property `{text}`",
            ContextualKeyword::Meta.as_str()
        ))
    })?;
    let range = s.span_from(start);
    Ok(s.expr(range, ExprKind::ImportMeta(property)))
}
