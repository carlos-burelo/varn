mod comments;
mod core;
mod literals;
mod operators;
mod templates;

pub(crate) struct Scanner<'a> {
    pub(super) src: &'a [u8],
    pub(super) pos: usize,
    pub(super) line_starts: Vec<usize>,
    pub(super) template_depth: usize,
    pub(super) brace_depth: Vec<usize>,
    pub(super) last_kind: u32,
    pub diagnostics: varn_core::DiagnosticBag,
}

impl<'a> Scanner<'a> {
    pub(crate) fn new(src: &'a [u8]) -> Self {
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
            diagnostics: varn_core::DiagnosticBag::new(),
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
            let c = self.src[self.pos];
            self.pos += 1;
            c
        } else {
            0
        }
    }

    #[inline]
    pub(super) fn match_byte(&mut self, expected: u8) -> bool {
        if !self.is_eof() && self.src[self.pos] == expected {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    pub(super) fn location(&self) -> (u32, u32) {
        self.location_at(self.pos)
    }

    pub(super) fn location_at(&self, pos: usize) -> (u32, u32) {
        let mut lo = 0usize;
        let mut hi = self.line_starts.len().saturating_sub(1);
        let mut line_idx = 0usize;
        while lo <= hi {
            let mid = (lo + hi) / 2;
            if self.line_starts[mid] <= pos {
                line_idx = mid;
                lo = mid + 1;
            } else {
                if mid == 0 {
                    break;
                }
                hi = mid - 1;
            }
        }
        let line = (line_idx + 1) as u32;
        let col = (pos - self.line_starts[line_idx]) as u32;
        (line, col)
    }
}
