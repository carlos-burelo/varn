#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexicalContext {
    Code,
    Comment,
    Text,
}

enum Frame {
    Template,
    Interpolation(u32),
}

pub fn lexical_context(source: &str, offset: usize) -> LexicalContext {
    let bytes = source.as_bytes();
    let end = offset.min(bytes.len());
    let mut stack: Vec<Frame> = Vec::new();
    let mut i = 0;

    while i < end {
        match stack.last_mut() {
            Some(Frame::Template) => {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'`' {
                    stack.pop();
                } else if bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'{') {
                    stack.push(Frame::Interpolation(0));
                    i += 1;
                }
                i += 1;
            }
            _ => {
                let rest = &bytes[i..];
                if rest.starts_with(b"//") {
                    let line_end = rest
                        .iter()
                        .position(|b| *b == b'\n')
                        .map_or(bytes.len(), |p| i + p);
                    if end <= line_end {
                        return LexicalContext::Comment;
                    }
                    i = line_end;
                } else if rest.starts_with(b"/*") {
                    match find(rest, b"*/", 2) {
                        Some(close) if i + close + 2 <= end => i += close + 2,
                        Some(_) | None => return LexicalContext::Comment,
                    }
                } else if rest.starts_with(b"\"\"\"") {
                    match find(rest, b"\"\"\"", 3) {
                        Some(close) if i + close + 3 <= end => i += close + 3,
                        Some(_) | None => return LexicalContext::Text,
                    }
                } else if bytes[i] == b'"' || bytes[i] == b'\'' {
                    let quote = bytes[i];
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j] != quote && bytes[j] != b'\n' {
                        if bytes[j] == b'\\' {
                            j += 1;
                        }
                        j += 1;
                    }
                    if j >= end {
                        return LexicalContext::Text;
                    }
                    i = j + 1;
                } else if bytes[i] == b'`' {
                    stack.push(Frame::Template);
                    i += 1;
                } else {
                    if let Some(Frame::Interpolation(depth)) = stack.last_mut() {
                        if bytes[i] == b'{' {
                            *depth += 1;
                        } else if bytes[i] == b'}' {
                            if *depth == 0 {
                                stack.pop();
                            } else {
                                *depth -= 1;
                            }
                        }
                    }
                    i += 1;
                }
            }
        }
    }

    match stack.last() {
        Some(Frame::Template) => LexicalContext::Text,
        Some(Frame::Interpolation(_)) | None => LexicalContext::Code,
    }
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|p| p + from)
}
