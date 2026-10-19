use crate::stream::TokenStream;
use varn_core::ast::{ExprId, ExprKind, MetaProperty};
use varn_core::TokenKind;

pub(super) fn parse_import_meta(s: &mut TokenStream) -> Result<ExprId, String> {
    let start = s.range();
    s.expect(TokenKind::Import)?;
    s.expect(TokenKind::Dot)?;
    let meta = s.expect_id()?;
    if s.interner.resolve(meta) != MetaProperty::OBJECT {
        return Err(format!("expected `import.{}`", MetaProperty::OBJECT));
    }
    s.expect(TokenKind::Dot)?;
    let name = s.expect_id()?;
    let text = s.interner.resolve(name);
    let property = MetaProperty::of(text)
        .ok_or_else(|| format!("unknown import.{} property `{text}`", MetaProperty::OBJECT))?;
    let range = s.span_from(start);
    Ok(s.expr(range, ExprKind::ImportMeta(property)))
}
