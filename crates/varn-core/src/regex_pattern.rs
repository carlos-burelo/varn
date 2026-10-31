use std::fmt;

const CASE_INSENSITIVE: char = 'i';
const MULTI_LINE: char = 'm';
const DOT_MATCHES_NEWLINE: char = 's';
const IGNORE_WHITESPACE: char = 'x';

pub const FLAGS: [char; 4] = [
    CASE_INSENSITIVE,
    MULTI_LINE,
    DOT_MATCHES_NEWLINE,
    IGNORE_WHITESPACE,
];

pub const MODULE: &str = "core:types/regex";
pub const CLASS: &str = "Regex";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    UnknownFlag(char),
    Invalid(String),
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFlag(flag) => write!(
                f,
                "unknown regex flag '{flag}': the flags are i, m, s and x; matching is always global"
            ),
            Self::Invalid(why) => write!(f, "Invalid regex pattern: {why}"),
        }
    }
}

pub fn compile(pattern: &str, flags: &str) -> Result<regex::Regex, PatternError> {
    if let Some(bad) = flags.chars().find(|c| !FLAGS.contains(c)) {
        return Err(PatternError::UnknownFlag(bad));
    }
    regex::RegexBuilder::new(pattern)
        .case_insensitive(flags.contains(CASE_INSENSITIVE))
        .multi_line(flags.contains(MULTI_LINE))
        .dot_matches_new_line(flags.contains(DOT_MATCHES_NEWLINE))
        .ignore_whitespace(flags.contains(IGNORE_WHITESPACE))
        .build()
        .map_err(|e| PatternError::Invalid(e.to_string()))
}
