use tower_lsp_f::lsp_types::Position;

pub fn byte_offset(text: &str, pos: Position) -> usize {
    let line_start = line_start(text, pos.line);
    column_offset(&text[line_start..], pos.character) + line_start
}

fn line_start(text: &str, line: u32) -> usize {
    if line == 0 {
        return 0;
    }
    let mut seen = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == line {
                return i + 1;
            }
        }
    }
    text.len()
}

fn column_offset(line: &str, character: u32) -> usize {
    let mut units = 0u32;
    for (i, ch) in line.char_indices() {
        if units >= character || ch == '\n' || ch == '\r' {
            return i;
        }
        units += ch.len_utf16() as u32;
    }
    line.len()
}
