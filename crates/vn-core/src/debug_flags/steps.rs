#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Complete,
    Hover,
    Definition,
    Declaration,
    Highlight,
    Signature,
}

impl Verb {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "complete" => Some(Verb::Complete),
            "hover" => Some(Verb::Hover),
            "definition" => Some(Verb::Definition),
            "declaration" => Some(Verb::Declaration),
            "highlight" => Some(Verb::Highlight),
            "signature" => Some(Verb::Signature),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Verb::Complete => "complete",
            Verb::Hover => "hover",
            Verb::Definition => "definition",
            Verb::Declaration => "declaration",
            Verb::Highlight => "highlight",
            Verb::Signature => "signature",
        }
    }

    pub fn all() -> Vec<Verb> {
        vec![
            Verb::Complete,
            Verb::Hover,
            Verb::Definition,
            Verb::Declaration,
            Verb::Highlight,
            Verb::Signature,
        ]
    }

    pub fn list() -> &'static str {
        "complete,hover,definition,declaration,highlight,signature"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Ge,
    Eq,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Predicate {
    Branch(String),
    Count(Cmp, usize),
    Has(String),
    Missing(String),
    Found,
    None_,
}

impl Predicate {
    fn parse(s: &str) -> Option<Self> {
        if let Some(branch) = s.strip_prefix("branch=") {
            if branch.is_empty() {
                return None;
            }
            return Some(Predicate::Branch(branch.to_owned()));
        }
        if let Some(rest) = s.strip_prefix("count>=") {
            return rest
                .parse::<usize>()
                .ok()
                .map(|n| Predicate::Count(Cmp::Ge, n));
        }
        if let Some(rest) = s.strip_prefix("count==") {
            return rest
                .parse::<usize>()
                .ok()
                .map(|n| Predicate::Count(Cmp::Eq, n));
        }
        if let Some(label) = s.strip_prefix("has=") {
            if label.is_empty() {
                return None;
            }
            return Some(Predicate::Has(label.to_owned()));
        }
        if let Some(label) = s.strip_prefix("missing=") {
            if label.is_empty() {
                return None;
            }
            return Some(Predicate::Missing(label.to_owned()));
        }
        match s {
            "found" => Some(Predicate::Found),
            "none" => Some(Predicate::None_),
            _ => None,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Predicate::Branch(b) => format!("branch={b}"),
            Predicate::Count(Cmp::Ge, n) => format!("count>={n}"),
            Predicate::Count(Cmp::Eq, n) => format!("count=={n}"),
            Predicate::Has(l) => format!("has={l}"),
            Predicate::Missing(l) => format!("missing={l}"),
            Predicate::Found => "found".to_owned(),
            Predicate::None_ => "none".to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub line: u32,
    pub col: u32,
    pub typed: Option<String>,
    pub ask: Vec<Verb>,
    pub expects: Vec<Predicate>,
}

impl Default for Step {
    fn default() -> Self {
        Step {
            line: 0,
            col: 0,
            typed: None,
            ask: Verb::all(),
            expects: Vec::new(),
        }
    }
}

fn tokenize(text: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    for c in text.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
            has_token = true;
        } else if c.is_whitespace() && !in_quotes {
            if has_token {
                tokens.push(std::mem::take(&mut current));
                has_token = false;
            }
        } else {
            current.push(c);
            has_token = true;
        }
    }
    if in_quotes {
        return None;
    }
    if has_token {
        tokens.push(current);
    }
    Some(tokens)
}

fn parse_pos(s: &str) -> Option<(u32, u32)> {
    let (l, c) = s.split_once(':')?;
    let line: u32 = l.parse().ok()?;
    let col: u32 = c.parse().ok()?;
    Some((line.saturating_sub(1), col.saturating_sub(1)))
}

pub fn parse_step(text: &str) -> Result<Step, String> {
    let tokens = tokenize(text).ok_or_else(|| format!("unterminated quote in step {text:?}"))?;
    let mut tokens = tokens.into_iter();
    let pos = tokens.next().ok_or_else(|| "empty step".to_string())?;
    if pos.contains('=') {
        return Err(format!("first token must be Ln:Col, got {pos:?}"));
    }
    let (line, col) = parse_pos(&pos)
        .ok_or_else(|| format!("invalid position {pos:?} (expected Ln:Col, 1-based)"))?;
    let mut step = Step {
        line,
        col,
        ..Step::default()
    };
    let mut ask_seen = false;
    for token in tokens {
        let (key, value) = token
            .split_once('=')
            .ok_or_else(|| format!("invalid pair {token:?} (expected key=value)"))?;
        match key {
            "type" => {
                if step.typed.is_some() {
                    return Err("duplicate type=".to_string());
                }
                step.typed = Some(value.to_owned());
            }
            "ask" => {
                if !ask_seen {
                    ask_seen = true;
                    step.ask = Vec::new();
                }
                if value.is_empty() {
                    return Err("ask= needs at least one verb".to_string());
                }
                step.ask.push(
                    Verb::parse(value).ok_or_else(|| {
                        format!("unknown verb {value:?} (verbs: {})", Verb::list())
                    })?,
                );
            }
            "expect" => {
                step.expects.push(
                    Predicate::parse(value).ok_or_else(|| {
                        format!(
                            "invalid expect {value:?} (branch=X, count>=N, count==N, has=X, missing=X, found, none)"
                        )
                    })?,
                );
            }
            _ => {
                return Err(format!("unknown key {key:?} (keys: type, ask, expect)"));
            }
        }
    }
    Ok(step)
}
