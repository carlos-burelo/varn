use crate::token_kind::{
    AMP, AMPAMP, AMPAMPEQ, AMPEQ, ARROW, AT, BACKSLASH, BACKTICK, BANG, BANGEQ, CARET, CARETEQ,
    COLON, COLONCOLON, COMMA, DOLLAR, DOT, DOTDOT, DOTDOTDOT, DOTDOTEQ, EQ, EQEQ, FATARROW, GTEQ,
    GTGT, GTGTEQ, GTGTGT, GTGTGTEQ, HASH, LANGLE, LBRACE, LBRACKET, LPAREN, LTEQ, LTLT, LTLTEQ,
    MINUS, MINUSEQ, MINUSMINUS, NEWLINE, PERCENT, PERCENTEQ, PIPE, PIPEEQ, PIPEGT, PIPEPIPE,
    PIPEPIPEEQ, PLUS, PLUSEQ, PLUSPLUS, QUESTION, QUESTIONDOT, QUESTIONLBRACKET, QUESTIONQUESTION,
    QUESTIONQUESTIONEQ, RANGLE, RBRACE, RBRACKET, RPAREN, SEMICOLON, SLASH, SLASHEQ, STAR, STAREQ,
    STARSTAR, STARSTAREQ, TILDE, UNKNOWN,
};
use crate::token_record::{push_token, TokenRecord};

impl super::Scanner<'_> {
    pub(super) fn scan_operator(&mut self, tokens: &mut Vec<TokenRecord>, lexemes: &mut Vec<u8>) {
        let start = self.pos;
        let (sl, sc) = self.location();
        // advance() now tracks line/col
        let c = self.advance();

        let kind = match c {
            b'(' => LPAREN,
            b')' => RPAREN,
            b'{' => LBRACE,
            b'}' => RBRACE,
            b'[' => LBRACKET,
            b']' => RBRACKET,
            b';' => SEMICOLON,
            b',' => COMMA,
            b'@' => AT,
            b'#' => HASH,
            b'~' => TILDE,
            b'`' => BACKTICK,
            b'$' => DOLLAR,
            b'\\' => BACKSLASH,
            b'\n' => NEWLINE,
            b'.' => {
                if self.peek(0) == b'.' {
                    // '.' — never newline
                    self.cur_col += 1;
                    self.pos += 1;
                    if self.match_byte(b'.') {
                        DOTDOTDOT
                    } else if self.match_byte(b'=') {
                        DOTDOTEQ
                    } else {
                        DOTDOT
                    }
                } else {
                    DOT
                }
            }
            b':' => {
                if self.match_byte(b':') {
                    COLONCOLON
                } else {
                    COLON
                }
            }
            b'?' => {
                if self.peek(0) == b'?' {
                    self.cur_col += 1;
                    self.pos += 1;
                    if self.match_byte(b'=') {
                        QUESTIONQUESTIONEQ
                    } else {
                        QUESTIONQUESTION
                    }
                } else if self.peek(0) == b'.' {
                    self.cur_col += 1;
                    self.pos += 1;
                    if self.match_byte(b'[') {
                        QUESTIONLBRACKET
                    } else {
                        QUESTIONDOT
                    }
                } else {
                    QUESTION
                }
            }
            b'+' => {
                if self.match_byte(b'+') {
                    PLUSPLUS
                } else if self.match_byte(b'=') {
                    PLUSEQ
                } else {
                    PLUS
                }
            }
            b'-' => {
                if self.match_byte(b'-') {
                    MINUSMINUS
                } else if self.match_byte(b'=') {
                    MINUSEQ
                } else if self.match_byte(b'>') {
                    ARROW
                } else {
                    MINUS
                }
            }
            b'*' => {
                if self.peek(0) == b'*' {
                    self.cur_col += 1;
                    self.pos += 1;
                    if self.match_byte(b'=') {
                        STARSTAREQ
                    } else {
                        STARSTAR
                    }
                } else if self.match_byte(b'=') {
                    STAREQ
                } else {
                    STAR
                }
            }
            b'/' => {
                if self.match_byte(b'=') {
                    SLASHEQ
                } else {
                    SLASH
                }
            }
            b'%' => {
                if self.match_byte(b'=') {
                    PERCENTEQ
                } else {
                    PERCENT
                }
            }
            b'&' => {
                if self.peek(0) == b'&' {
                    self.cur_col += 1;
                    self.pos += 1;
                    if self.match_byte(b'=') {
                        AMPAMPEQ
                    } else {
                        AMPAMP
                    }
                } else if self.match_byte(b'=') {
                    AMPEQ
                } else {
                    AMP
                }
            }
            b'|' => {
                if self.peek(0) == b'|' {
                    self.cur_col += 1;
                    self.pos += 1;
                    if self.match_byte(b'=') {
                        PIPEPIPEEQ
                    } else {
                        PIPEPIPE
                    }
                } else if self.peek(0) == b'>' {
                    self.cur_col += 1;
                    self.pos += 1;
                    PIPEGT
                } else if self.match_byte(b'=') {
                    PIPEEQ
                } else {
                    PIPE
                }
            }
            b'^' => {
                if self.match_byte(b'=') {
                    CARETEQ
                } else {
                    CARET
                }
            }
            b'<' => {
                if self.peek(0) == b'<' {
                    self.cur_col += 1;
                    self.pos += 1;
                    if self.match_byte(b'=') {
                        LTLTEQ
                    } else {
                        LTLT
                    }
                } else if self.match_byte(b'=') {
                    LTEQ
                } else {
                    LANGLE
                }
            }
            b'>' => {
                if self.peek(0) == b'>' {
                    self.cur_col += 1;
                    self.pos += 1;
                    if self.peek(0) == b'>' {
                        self.cur_col += 1;
                        self.pos += 1;
                        if self.match_byte(b'=') {
                            GTGTGTEQ
                        } else {
                            GTGTGT
                        }
                    } else if self.match_byte(b'=') {
                        GTGTEQ
                    } else {
                        GTGT
                    }
                } else if self.match_byte(b'=') {
                    GTEQ
                } else {
                    RANGLE
                }
            }
            b'=' => {
                if self.peek(0) == b'=' {
                    self.cur_col += 1;
                    self.pos += 1;
                    self.match_byte(b'=');
                    EQEQ
                } else if self.match_byte(b'>') {
                    FATARROW
                } else {
                    EQ
                }
            }
            b'!' => {
                if self.peek(0) == b'=' {
                    self.cur_col += 1;
                    self.pos += 1;
                    self.match_byte(b'=');
                    BANGEQ
                } else {
                    BANG
                }
            }
            _ => UNKNOWN,
        };

        let (el, ec) = self.location();
        push_token(
            tokens,
            lexemes,
            self.src,
            kind,
            sl,
            sc,
            start as u32,
            el,
            ec,
            self.pos as u32,
            start,
            self.pos,
        );
    }
}
