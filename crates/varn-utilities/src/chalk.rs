use std::fmt::{self, Display, Formatter, Write as FmtWrite};

pub struct Chalk {
    text: String,
    codes: String,
}

impl Chalk {
    fn new(text: impl Display) -> Self {
        Self { text: text.to_string(), codes: String::new() }
    }

    fn push(mut self, code: &str) -> Self {
        self.codes.push_str(code);
        self
    }

    // Foreground colors
    pub fn red(self) -> Self       { self.push("\x1b[31m") }
    pub fn green(self) -> Self     { self.push("\x1b[32m") }
    pub fn yellow(self) -> Self    { self.push("\x1b[33m") }
    pub fn blue(self) -> Self      { self.push("\x1b[34m") }
    pub fn magenta(self) -> Self   { self.push("\x1b[35m") }
    pub fn cyan(self) -> Self      { self.push("\x1b[36m") }
    pub fn white(self) -> Self     { self.push("\x1b[37m") }

    // Modifiers
    pub fn bold(self) -> Self      { self.push("\x1b[1m") }
    pub fn dim(self) -> Self       { self.push("\x1b[2m") }
    pub fn italic(self) -> Self    { self.push("\x1b[3m") }
    pub fn underline(self) -> Self { self.push("\x1b[4m") }
    pub fn strike(self) -> Self    { self.push("\x1b[9m") }
}

impl Display for Chalk {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.codes.is_empty() {
            f.write_str(&self.text)
        } else {
            f.write_str(&self.codes)?;
            f.write_str(&self.text)?;
            f.write_str("\x1b[0m")
        }
    }
}

/// Entry point — `chalk("text").red().bold()`
pub fn chalk(text: impl Display) -> Chalk {
    Chalk::new(text)
}

/// Style a pre-formatted `format_args!` without intermediate allocation.
/// `chalk_fmt(format_args!("x={}", val)).cyan()`
pub fn chalk_fmt(args: fmt::Arguments<'_>) -> Chalk {
    let mut buf = String::new();
    let _ = buf.write_fmt(args);
    Chalk { text: buf, codes: String::new() }
}
