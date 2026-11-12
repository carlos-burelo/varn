pub fn byte_column(source: &str, line: u32, utf16_column: u32) -> u32 {
    let Some(text) = source.lines().nth(line as usize) else {
        return utf16_column;
    };
    let mut units = 0u32;
    for (byte, ch) in text.char_indices() {
        if units >= utf16_column {
            return byte as u32;
        }
        units += ch.len_utf16() as u32;
    }
    text.len() as u32
}

pub fn utf16_column(source: &str, line: u32, byte_column: u32) -> u32 {
    let Some(text) = source.lines().nth(line as usize) else {
        return byte_column;
    };
    let mut units = 0u32;
    for (byte, ch) in text.char_indices() {
        if byte as u32 >= byte_column {
            break;
        }
        units += ch.len_utf16() as u32;
    }
    units
}
