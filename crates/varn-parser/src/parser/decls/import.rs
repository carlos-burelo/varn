use crate::stream::TokenStream;
use varn_core::ast::decl::ImportSpecifier;
use varn_core::ast::ImportDecl;
use varn_core::TokenKind;

pub fn parse_import_decl(s: &mut TokenStream) -> Result<ImportDecl, String> {
    let range = s.range();
    s.expect(TokenKind::Import)?;

    let is_type = s.check(TokenKind::Type)
        && matches!(
            s.peek_kind(1),
            TokenKind::LBrace | TokenKind::Star | TokenKind::Identifier
        )
        && {
            s.advance();
            true
        };

    let mut specifiers = vec![];

    if s.check(TokenKind::Str) {
        let source = s.consume_lexeme();
        let full_range = s.span_from(range);
        return Ok(ImportDecl {
            ast_id: s.next_ast_id(),
            specifiers,
            source,
            is_type: false,
            range: full_range,
        });
    }

    if s.kind().can_be_identifier()
        && (s.peek_kind(1) == TokenKind::Comma
            || s.peek_kind(1) == TokenKind::From
            || s.peek_kind(1) == TokenKind::LBrace)
    {
        let spec_start = s.range();
        let local = s.consume_lexeme();
        let spec_range = s.span_from(spec_start);
        specifiers.push(ImportSpecifier::Default {
            local,
            range: spec_range,
        });
        s.eat(TokenKind::Comma);
    }

    if s.eat(TokenKind::Star) {
        let spec_start = s.range();
        s.expect(TokenKind::As)?;
        let local = s.expect_id()?;
        let spec_range = s.span_from(spec_start);
        specifiers.push(ImportSpecifier::Namespace {
            local,
            range: spec_range,
        });
    } else if s.check(TokenKind::LBrace) {
        s.advance();
        while !s.check(TokenKind::RBrace) && !s.is_eof() {
            let spec_range = s.range();
            let imported = s.consume_lexeme();
            let local = if s.eat(TokenKind::As) {
                s.consume_lexeme()
            } else {
                imported
            };
            let full_spec_range = s.span_from(spec_range);
            specifiers.push(ImportSpecifier::Named {
                local,
                imported,
                range: full_spec_range,
            });
            if !s.eat(TokenKind::Comma) {
                break;
            }
        }
        s.expect(TokenKind::RBrace)?;
    }

    s.expect(TokenKind::From)?;
    let source = s.consume_str();
    let source = s.interner.intern(&source);
    let full_range = s.span_from(range);

    Ok(ImportDecl {
        ast_id: s.next_ast_id(),
        specifiers,
        source,
        is_type,
        range: full_range,
    })
}
