#[derive(Clone, Debug, Default)]
pub struct DocParam {
    pub type_str: Option<String>,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Default)]
pub struct DocReturn {
    pub type_str: Option<String>,
    pub description: String,
}

#[derive(Clone, Debug, Default)]
pub struct DocThrows {
    pub type_str: Option<String>,
    pub description: String,
}

#[derive(Clone, Debug, Default)]
pub struct DocComment {
    pub description: String,
    pub params: Vec<DocParam>,
    pub returns: Option<DocReturn>,
    pub throws: Vec<DocThrows>,
    pub example: Option<String>,
    pub deprecated: Option<String>,
    pub since: Option<String>,
    pub see: Vec<String>,
    pub tags: Vec<(String, String)>,
}

impl DocComment {
    pub fn parse(raw: &str) -> Self {
        let lines = clean_lines(raw);
        let text = lines.join("\n");

        let mut doc = DocComment::default();
        let mut current_tag: Option<String> = None;
        let mut current_body: String = String::new();

        for line in text.lines() {
            let trimmed = line.trim();

            if let Some(rest) = trimmed.strip_prefix('@') {
                flush_tag(&mut doc, current_tag.take(), &current_body);
                current_body.clear();

                let (tag_name, tag_rest) = split_first_word(rest);
                current_tag = Some(tag_name.to_ascii_lowercase());
                current_body = tag_rest.trim().to_owned();
            } else if current_tag.is_some() {
                if !current_body.is_empty() {
                    current_body.push('\n');
                }
                current_body.push_str(trimmed);
            } else {
                if !doc.description.is_empty() {
                    doc.description.push('\n');
                }
                doc.description.push_str(trimmed);
            }
        }
        flush_tag(&mut doc, current_tag, &current_body);
        doc.description = doc.description.trim_end_matches('\n').to_owned();
        doc
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::with_capacity(256);

        if !self.description.is_empty() {
            out.push_str(&self.description);
            out.push_str("\n\n");
        }

        if let Some(msg) = &self.deprecated {
            out.push_str("⚠️ **Deprecated**");
            if !msg.is_empty() {
                out.push_str(": ");
                out.push_str(msg);
            }
            out.push_str("\n\n");
        }

        if let Some(ver) = &self.since {
            out.push_str(&format!("*Since* `{ver}`\n\n"));
        }

        if !self.params.is_empty() {
            out.push_str("**Parameters**\n\n");
            for p in &self.params {
                out.push_str("- `");
                out.push_str(&p.name);
                out.push('`');
                if let Some(ts) = &p.type_str {
                    out.push_str(&format!(" `{ts}`"));
                }
                if !p.description.is_empty() {
                    out.push_str(" — ");
                    out.push_str(&p.description);
                }
                out.push('\n');
            }
            out.push('\n');
        }

        if let Some(ret) = &self.returns {
            out.push_str("**Returns**");
            if let Some(ts) = &ret.type_str {
                out.push_str(&format!(" `{ts}`"));
            }
            if !ret.description.is_empty() {
                out.push_str(" — ");
                out.push_str(&ret.description);
            }
            out.push_str("\n\n");
        }

        if !self.throws.is_empty() {
            out.push_str("**Throws**\n\n");
            for th in &self.throws {
                out.push_str("- ");
                if let Some(ts) = &th.type_str {
                    out.push_str(&format!("`{ts}`"));
                }
                if !th.description.is_empty() {
                    if th.type_str.is_some() {
                        out.push_str(" — ");
                    }
                    out.push_str(&th.description);
                }
                out.push('\n');
            }
            out.push('\n');
        }

        if !self.see.is_empty() {
            out.push_str("**See also:** ");
            let refs: Vec<&str> = self.see.iter().map(String::as_str).collect();
            out.push_str(&refs.join(", "));
            out.push_str("\n\n");
        }

        if let Some(ex) = &self.example {
            out.push_str("**Example**\n\n```Varn\n");
            out.push_str(ex.trim());
            out.push_str("\n```\n\n");
        }

        for (tag, val) in &self.tags {
            out.push_str(&format!("*@{tag}* {val}\n"));
        }

        out.trim_end().to_owned()
    }
}

fn clean_lines(raw: &str) -> Vec<String> {
    let mut lines: Vec<String> = raw
        .lines()
        .map(|line| {
            let t = line.trim();

            if let Some(rest) = t.strip_prefix('*') {
                rest.strip_prefix(' ').unwrap_or(rest).to_owned()
            } else {
                t.to_owned()
            }
        })
        .collect();

    while lines.first().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.remove(0);
    }

    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }

    lines
}

fn split_first_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(|c: char| c.is_ascii_whitespace()) {
        Some(pos) => (&s[..pos], &s[pos..]),
        None => (s, ""),
    }
}

fn take_braced_type(s: &str) -> (Option<String>, &str) {
    let s = s.trim_start();
    if !s.starts_with('{') {
        return (None, s);
    }
    match s.find('}') {
        Some(end) => {
            let ty = s[1..end].trim().to_owned();
            let rest = s[end + 1..].trim_start();
            (Some(ty), rest)
        }
        None => (None, s),
    }
}

fn parse_param_body(body: &str) -> DocParam {
    let (type_str, after_type) = take_braced_type(body.trim());
    let (name, desc) = split_first_word(after_type);
    DocParam {
        type_str,
        name: name.trim_start_matches('-').trim().to_owned(),
        description: desc.trim_start_matches('-').trim().to_owned(),
    }
}

fn parse_typed_body(body: &str) -> (Option<String>, String) {
    let (type_str, rest) = take_braced_type(body.trim());
    let desc = rest.trim_start_matches('-').trim().to_owned();
    (type_str, desc)
}

fn flush_tag(doc: &mut DocComment, tag: Option<String>, body: &str) {
    let body = body.trim();
    match tag.as_deref() {
        None => {}
        Some("param" | "arg" | "argument") => {
            doc.params.push(parse_param_body(body));
        }
        Some("returns" | "return") => {
            let (type_str, description) = parse_typed_body(body);
            doc.returns = Some(DocReturn {
                type_str,
                description,
            });
        }
        Some("throws" | "exception") => {
            let (type_str, description) = parse_typed_body(body);
            doc.throws.push(DocThrows {
                type_str,
                description,
            });
        }
        Some("example") => {
            doc.example = Some(body.to_owned());
        }
        Some("deprecated") => {
            doc.deprecated = Some(body.to_owned());
        }
        Some("since") => {
            doc.since = Some(body.to_owned());
        }
        Some("see") => {
            if !body.is_empty() {
                doc.see.push(body.to_owned());
            }
        }
        Some(other) => {
            doc.tags.push((other.to_owned(), body.to_owned()));
        }
    }
}
