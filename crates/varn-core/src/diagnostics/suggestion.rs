use crate::source::SourceRange;

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub message: String,

    pub range: Option<SourceRange>,

    pub replacement: Option<String>,
}

impl Suggestion {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            range: None,
            replacement: None,
        }
    }

    pub fn with_range(mut self, range: SourceRange) -> Self {
        self.range = Some(range);
        self
    }

    pub fn replace(mut self, text: impl Into<String>) -> Self {
        self.replacement = Some(text.into());
        self
    }

    pub fn did_you_mean(name: &str, range: SourceRange) -> Self {
        Self {
            message: format!("Did you mean `{name}`?"),
            range: Some(range),
            replacement: Some(name.to_owned()),
        }
    }
}

impl std::fmt::Display for Suggestion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "💡 {}", self.message)
    }
}
