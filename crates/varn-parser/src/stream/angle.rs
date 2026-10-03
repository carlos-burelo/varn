use super::TokenStream;
use varn_core::TokenKind;

impl TokenStream {
    #[inline]
    pub fn check_rangle(&self) -> bool {
        self.split_count > 0
            || matches!(
                self.token().kind,
                TokenKind::RAngle | TokenKind::GtGt | TokenKind::GtGtGt
            )
    }

    pub fn eat_rangle(&mut self) -> bool {
        if self.split_count > 0 {
            self.split_count -= 1;
            return true;
        }
        match self.token().kind {
            TokenKind::RAngle => {
                self.pos += 1;
                true
            }
            TokenKind::GtGt => {
                self.split_count = 1;
                self.pos += 1;
                true
            }
            TokenKind::GtGtGt => {
                self.split_count = 2;
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    pub fn expect_rangle(&mut self) -> Result<(), String> {
        if self.eat_rangle() {
            Ok(())
        } else {
            let tok = self.token();
            let lex = tok.get_lexeme(&self.lexeme_buf);
            Err(format!(
                "Expected RAngle, got {:?} ({:?}) at {}:{}",
                tok.kind, lex, tok.range.start.line, tok.range.start.column
            ))
        }
    }

    #[inline]
    pub fn save(&self) -> (usize, u8) {
        (self.pos, self.split_count)
    }

    #[inline]
    pub fn restore(&mut self, state: (usize, u8)) {
        self.pos = state.0;
        self.split_count = state.1;
    }
}
