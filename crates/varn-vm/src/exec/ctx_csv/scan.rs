#[inline(always)]
pub(super) fn trim_bytes(mut b: &[u8]) -> &[u8] {
    while let Some((first, rest)) = b.split_first() {
        if *first == b' ' || *first == b'\t' {
            b = rest;
        } else {
            break;
        }
    }
    while let Some((last, rest)) = b.split_last() {
        if *last == b' ' || *last == b'\t' {
            b = rest;
        } else {
            break;
        }
    }
    b
}

pub(super) struct FastCsvParser<'a> {
    bytes: &'a [u8],
    pos: usize,
    delimiter: u8,
    trim: bool,
}

impl<'a> FastCsvParser<'a> {
    pub(super) fn new(bytes: &'a [u8], delimiter: u8, trim: bool) -> Self {
        Self {
            bytes,
            pos: 0,
            delimiter,
            trim,
        }
    }

    pub(super) fn next_row_into(
        &mut self,
        fields: &mut Vec<std::borrow::Cow<'a, str>>,
    ) -> Result<bool, String> {
        fields.clear();
        if self.pos >= self.bytes.len() {
            return Ok(false);
        }

        let mut field_start = self.pos;
        let mut in_quotes = false;
        let mut has_escapes = false;
        let mut escaped_buf = String::new();

        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];

            if in_quotes {
                if b == b'"' {
                    if self.pos + 1 < self.bytes.len() && self.bytes[self.pos + 1] == b'"' {
                        if !has_escapes {
                            has_escapes = true;
                            escaped_buf.clear();
                            escaped_buf.push_str(unsafe {
                                std::str::from_utf8_unchecked(
                                    &self.bytes[field_start + 1..self.pos],
                                )
                            });
                        }
                        escaped_buf.push('"');
                        self.pos += 2;
                    } else {
                        in_quotes = false;
                        self.pos += 1;
                    }
                } else {
                    if has_escapes {
                        escaped_buf.push(b as char);
                    }
                    self.pos += 1;
                }
            } else if b == b'"' && self.pos == field_start {
                in_quotes = true;
                self.pos += 1;
            } else if b == self.delimiter {
                let cell: std::borrow::Cow<'a, str> = if has_escapes {
                    std::borrow::Cow::Owned(std::mem::take(&mut escaped_buf))
                } else {
                    let mut slice = &self.bytes[field_start..self.pos];
                    if slice.starts_with(b"\"") && slice.ends_with(b"\"") && slice.len() >= 2 {
                        slice = &slice[1..slice.len() - 1];
                    } else if self.trim {
                        slice = trim_bytes(slice);
                    }
                    let s = std::str::from_utf8(slice).map_err(|e| e.to_string())?;
                    std::borrow::Cow::Borrowed(s)
                };
                fields.push(cell);
                has_escapes = false;
                self.pos += 1;
                field_start = self.pos;
            } else if b == b'\r' || b == b'\n' {
                let cell: std::borrow::Cow<'a, str> = if has_escapes {
                    std::borrow::Cow::Owned(std::mem::take(&mut escaped_buf))
                } else {
                    let mut slice = &self.bytes[field_start..self.pos];
                    if slice.starts_with(b"\"") && slice.ends_with(b"\"") && slice.len() >= 2 {
                        slice = &slice[1..slice.len() - 1];
                    } else if self.trim {
                        slice = trim_bytes(slice);
                    }
                    let s = std::str::from_utf8(slice).map_err(|e| e.to_string())?;
                    std::borrow::Cow::Borrowed(s)
                };
                fields.push(cell);
                if b == b'\r'
                    && self.pos + 1 < self.bytes.len()
                    && self.bytes[self.pos + 1] == b'\n'
                {
                    self.pos += 2;
                } else {
                    self.pos += 1;
                }
                return Ok(true);
            } else {
                if has_escapes {
                    escaped_buf.push(b as char);
                }
                self.pos += 1;
            }
        }

        if in_quotes {
            return Err("Unclosed quote in CSV data".to_string());
        }

        if field_start <= self.bytes.len() {
            let cell: std::borrow::Cow<'a, str> = if has_escapes {
                std::borrow::Cow::Owned(escaped_buf)
            } else {
                let mut slice = &self.bytes[field_start..self.pos];
                if slice.starts_with(b"\"") && slice.ends_with(b"\"") && slice.len() >= 2 {
                    slice = &slice[1..slice.len() - 1];
                } else if self.trim {
                    slice = trim_bytes(slice);
                }
                let s = std::str::from_utf8(slice).map_err(|e| e.to_string())?;
                std::borrow::Cow::Borrowed(s)
            };
            fields.push(cell);
        }

        Ok(true)
    }
}
