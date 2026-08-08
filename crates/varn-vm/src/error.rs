use std::fmt;

#[derive(Debug, Clone)]
pub struct FrameInfo {
    pub fn_name: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
    pub frames: Vec<FrameInfo>,

    pub thrown: Option<crate::value::VmValue>,
}

impl RuntimeError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            frames: Vec::new(),
            thrown: None,
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\x1b[1m\x1b[31mUncaught Runtime Error\x1b[0m: \x1b[1m{}\x1b[0m",
            self.message
        )?;
        if let Some(top) = self.frames.first() {
            if top.line > 0 && !top.file.is_empty() {
                if let Ok(content) = std::fs::read_to_string(&top.file) {
                    let line_idx = (top.line as usize).saturating_sub(1);
                    if let Some(src_line) = content.lines().nth(line_idx) {
                        let margin = top.line.to_string().len().max(1);
                        let gutter = " ".repeat(margin);
                        write!(
                            f,
                            "\n{gutter} \x1b[2m┌─\x1b[0m {file}:{line}\n{gutter} \x1b[2m│\x1b[0m\n\x1b[2m{line:>margin$} │\x1b[0m {src_line}\n{gutter} \x1b[2m└─\x1b[0m",
                            file = top.file,
                            line = top.line,
                            margin = margin,
                            src_line = src_line
                        )?;
                    }
                }
            }
        }
        if !self.frames.is_empty() {
            write!(f, "\n\x1b[1mStack Trace:\x1b[0m")?;
            for frame in &self.frames {
                write!(
                    f,
                    "\n    \x1b[34mat\x1b[0m {} \x1b[2m({}:{})\x1b[0m",
                    frame.fn_name, frame.file, frame.line
                )?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for RuntimeError {}

pub type VmResult<T> = Result<T, RuntimeError>;
