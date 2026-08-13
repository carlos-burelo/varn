mod comments;
mod core;
mod literals;
mod operators;
mod templates;

#[derive(Clone, Copy, Debug)]
pub struct LexerConfig {
    pub emit_doc_comments: bool,
}

impl Default for LexerConfig {
    fn default() -> Self {
        Self {
            emit_doc_comments: true,
        }
    }
}

pub(crate) struct Scanner<'a> {
    pub(super) src: &'a [u8],
    pub(super) pos: usize,
    pub(super) line_starts: Vec<usize>,
    pub(super) template_depth: usize,
    pub(super) brace_depth: Vec<usize>,
    pub(super) last_kind: u32,
    pub(super) config: LexerConfig,
    pub diagnostics: varn_core::DiagnosticBag,
    pub(super) cur_line: u32,
    pub(super) cur_col: u32,
}

impl<'a> Scanner<'a> {
    pub(crate) fn with_config(src: &'a [u8], config: LexerConfig) -> Self {
        let mut line_starts = Vec::with_capacity(64);
        line_starts.push(0usize);
        for (i, &b) in src.iter().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Scanner {
            src,
            pos: 0,
            line_starts,
            template_depth: 0,
            brace_depth: Vec::new(),
            last_kind: u32::MAX,
            config,
            diagnostics: varn_core::DiagnosticBag::new(),
            cur_line: 1,
            cur_col: 0,
        }
    }

    #[inline]
    pub(super) fn is_eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    #[inline]
    pub(super) fn peek(&self, offset: usize) -> u8 {
        let p = self.pos + offset;
        if p < self.src.len() {
            self.src[p]
        } else {
            0
        }
    }

    #[inline]
    pub(super) fn advance(&mut self) -> u8 {
        if self.pos < self.src.len() {
            let b = self.src[self.pos];
            self.pos += 1;
            if b == b'\n' {
                self.cur_line += 1;
                self.cur_col = 0;
            } else {
                self.cur_col += 1;
            }
            b
        } else {
            0
        }
    }

    #[inline]
    pub(super) fn match_byte(&mut self, expected: u8) -> bool {
        if !self.is_eof() && self.src[self.pos] == expected {
            let b = self.src[self.pos];
            self.pos += 1;
            if b == b'\n' {
                self.cur_line += 1;
                self.cur_col = 0;
            } else {
                self.cur_col += 1;
            }
            true
        } else {
            false
        }
    }

    #[inline]
    pub(super) fn advance_byte(&mut self) {
        if self.pos < self.src.len() {
            let b = self.src[self.pos];
            self.pos += 1;
            if b == b'\n' {
                self.cur_line += 1;
                self.cur_col = 0;
            } else {
                self.cur_col += 1;
            }
        }
    }

    #[inline]
    pub(super) fn advance_bytes(&mut self, n: usize) {
        for _ in 0..n {
            self.advance_byte();
        }
    }

    #[inline]
    pub(super) fn location(&self) -> (u32, u32) {
        (self.cur_line, self.cur_col)
    }

    pub(super) fn location_at(&self, pos: usize) -> (u32, u32) {
        let line_idx = self
            .line_starts
            .partition_point(|&s| s <= pos)
            .saturating_sub(1);
        let line = (line_idx + 1) as u32;
        let col = (pos - self.line_starts[line_idx]) as u32;
        (line, col)
    }

    #[inline]
    pub(super) fn skip_whitespace(&mut self) {
        while self.pos + 8 <= self.src.len() {
            let chunk = u64::from_le_bytes(self.src[self.pos..self.pos + 8].try_into().unwrap());
            if chunk == 0x2020_2020_2020_2020 {
                self.pos += 8;
                self.cur_col += 8;
                continue;
            }
            break;
        }

        while self.pos < self.src.len() {
            let b = self.src[self.pos];
            if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
                self.advance_byte();
            } else {
                break;
            }
        }
    }
}
