use crate::exec::ExecCtx;
use crate::heap::HeapObj;
use varn_types::value::root_shape;
use varn_types::VmValue;

pub(crate) fn parse_csv(
    ctx: &mut ExecCtx,
    text: &str,
    delimiter: u8,
    has_header: bool,
    trim: bool,
) -> Result<VmValue, String> {
    let mut parser = FastCsvParser::new(text.as_bytes(), delimiter, trim);
    let mut row_buf: Vec<std::borrow::Cow<'_, str>> = Vec::with_capacity(32);

    if !parser.next_row_into(&mut row_buf)? || row_buf.is_empty() {
        return Ok(ctx.heap.alloc_array_vm(Vec::new()));
    }

    if has_header {
        let mut shape = root_shape();
        for col_name in &row_buf {
            shape = shape.transition(col_name.as_ref().into());
        }
        let num_cols = row_buf.len();

        let est_rows = (text.len() / (num_cols * 8).max(16)).clamp(16, 131072);
        let mut out_objects: Vec<VmValue> = Vec::with_capacity(est_rows);
        let mut field_values: Vec<VmValue> = Vec::with_capacity(num_cols);

        while parser.next_row_into(&mut row_buf)? {
            if row_buf.is_empty() || (row_buf.len() == 1 && row_buf[0].is_empty()) {
                continue;
            }
            field_values.clear();
            for cell in &row_buf[..row_buf.len().min(num_cols)] {
                field_values.push(parse_cell_value(ctx, cell.as_ref()));
            }
            while field_values.len() < num_cols {
                field_values.push(VmValue::null());
            }
            let obj = ctx
                .heap
                .alloc_object_with_shape_slice(&shape, &field_values);
            out_objects.push(obj);
        }

        Ok(ctx.heap.alloc_array_vm(out_objects))
    } else {
        let est_rows = (text.len() / 32).clamp(16, 131072);
        let mut out_rows: Vec<VmValue> = Vec::with_capacity(est_rows);

        let mut first_vals: Vec<VmValue> = Vec::with_capacity(row_buf.len());
        for cell in &row_buf {
            first_vals.push(parse_cell_value(ctx, cell.as_ref()));
        }
        out_rows.push(ctx.heap.alloc_array_vm(first_vals));

        while parser.next_row_into(&mut row_buf)? {
            if row_buf.is_empty() || (row_buf.len() == 1 && row_buf[0].is_empty()) {
                continue;
            }
            let mut row_vals: Vec<VmValue> = Vec::with_capacity(row_buf.len());
            for cell in &row_buf {
                row_vals.push(parse_cell_value(ctx, cell.as_ref()));
            }
            out_rows.push(ctx.heap.alloc_array_vm(row_vals));
        }

        Ok(ctx.heap.alloc_array_vm(out_rows))
    }
}

#[inline(always)]
fn trim_bytes(mut b: &[u8]) -> &[u8] {
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

#[inline]
fn parse_cell_value(ctx: &mut ExecCtx, s: &str) -> VmValue {
    if s.is_empty() {
        return VmValue::null();
    }
    if s == "true" {
        return VmValue::bool_true();
    }
    if s == "false" {
        return VmValue::bool_false();
    }
    if s == "null" {
        return VmValue::null();
    }

    // Fast integer parsing
    let bytes = s.as_bytes();
    let mut idx = 0;
    let neg = if bytes[0] == b'-' {
        idx = 1;
        true
    } else {
        false
    };
    if idx < bytes.len() && bytes[idx..].iter().all(|&b| b.is_ascii_digit()) {
        let mut int_val: i64 = 0;
        let mut overflow = false;
        for &b in &bytes[idx..] {
            if let Some(next) = int_val
                .checked_mul(10)
                .and_then(|v| v.checked_add((b - b'0') as i64))
            {
                int_val = next;
            } else {
                overflow = true;
                break;
            }
        }
        if !overflow {
            return VmValue::from_int(if neg { -int_val } else { int_val });
        }
    }

    // Try parsing as float
    if let Ok(f) = s.parse::<f64>() {
        if f.is_finite() {
            return VmValue::from_f64(f);
        }
    }

    // Short string optimization (SSO)
    if let Some(sso) = VmValue::try_from_sso(s) {
        return sso;
    }

    ctx.heap.alloc_str_dynamic(s)
}

struct FastCsvParser<'a> {
    bytes: &'a [u8],
    pos: usize,
    delimiter: u8,
    trim: bool,
}

impl<'a> FastCsvParser<'a> {
    fn new(bytes: &'a [u8], delimiter: u8, trim: bool) -> Self {
        Self {
            bytes,
            pos: 0,
            delimiter,
            trim,
        }
    }

    fn next_row_into(
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

pub(crate) fn stringify_csv(
    ctx: &mut ExecCtx,
    value: VmValue,
    delimiter: u8,
) -> Result<String, String> {
    if !value.is_heap() {
        return Err("CSV stringify expects an array of objects or rows".to_string());
    }

    let delim_char = delimiter as char;
    let mut out = String::new();

    match ctx.heap.get(value.as_heap_idx()) {
        Some(HeapObj::Array(arr)) => {
            let repr = arr.repr();
            let items = match repr {
                varn_types::ArrayRepr::Boxed(v) => v,
                _ => return Err("CSV stringify expects a boxed array".to_string()),
            };

            if items.is_empty() {
                return Ok(String::new());
            }

            let first = items[0];
            if !first.is_heap() {
                return Err("CSV items must be objects or array rows".to_string());
            }

            match ctx.heap.get(first.as_heap_idx()) {
                Some(HeapObj::Object(first_obj) | HeapObj::Record(first_obj)) => {
                    let shape = first_obj.borrow().shape().clone();
                    let prop_names: Vec<String> = {
                        let mut names_with_slots: Vec<(String, usize)> = shape
                            .property_names
                            .iter()
                            .map(|(k, &slot)| (k.to_string(), slot))
                            .collect();
                        names_with_slots.sort_by_key(|(_, slot)| *slot);
                        names_with_slots.into_iter().map(|(k, _)| k).collect()
                    };

                    // Write Header
                    for (i, name) in prop_names.iter().enumerate() {
                        if i > 0 {
                            out.push(delim_char);
                        }
                        write_csv_cell(&mut out, name, delim_char);
                    }
                    out.push('\n');

                    // Write Rows
                    for item in items {
                        if !item.is_heap() {
                            continue;
                        }
                        if let Some(HeapObj::Object(obj) | HeapObj::Record(obj)) =
                            ctx.heap.get(item.as_heap_idx())
                        {
                            let o = obj.borrow();
                            let inline = o.inline_slice();
                            for (slot, _) in prop_names.iter().enumerate() {
                                if slot > 0 {
                                    out.push(delim_char);
                                }
                                let val = if slot < inline.len() {
                                    inline[slot].get()
                                } else {
                                    o.field_at(slot).unwrap_or_else(VmValue::null)
                                };
                                write_vm_value_csv(&mut out, ctx, val, delim_char);
                            }
                            out.push('\n');
                        }
                    }
                }
                Some(HeapObj::Array(_first_row)) => {
                    for item in items {
                        if !item.is_heap() {
                            continue;
                        }
                        if let Some(HeapObj::Array(row_arr)) = ctx.heap.get(item.as_heap_idx()) {
                            match row_arr.repr() {
                                varn_types::ArrayRepr::Boxed(row_items) => {
                                    for (i, &cell) in row_items.iter().enumerate() {
                                        if i > 0 {
                                            out.push(delim_char);
                                        }
                                        write_vm_value_csv(&mut out, ctx, cell, delim_char);
                                    }
                                    out.push('\n');
                                }
                                varn_types::ArrayRepr::I64(row_items) => {
                                    for (i, &cell) in row_items.iter().enumerate() {
                                        if i > 0 {
                                            out.push(delim_char);
                                        }
                                        out.push_str(&cell.to_string());
                                    }
                                    out.push('\n');
                                }
                                varn_types::ArrayRepr::F64(row_items) => {
                                    for (i, &cell) in row_items.iter().enumerate() {
                                        if i > 0 {
                                            out.push(delim_char);
                                        }
                                        if cell.is_finite() {
                                            out.push_str(ryu::Buffer::new().format(cell));
                                        }
                                    }
                                    out.push('\n');
                                }
                            }
                        }
                    }
                }
                _ => return Err("Unsupported row type for CSV stringify".to_string()),
            }

            Ok(out)
        }
        _ => Err("CSV stringify expects an array".to_string()),
    }
}

fn write_vm_value_csv(out: &mut String, ctx: &ExecCtx, val: VmValue, delimiter: char) {
    if val.is_null() {
        return;
    }
    if val.is_bool() {
        out.push_str(if val.as_bool() { "true" } else { "false" });
        return;
    }
    if val.is_int() {
        out.push_str(&val.as_int().to_string());
        return;
    }
    if val.is_f64() {
        let f = val.as_f64();
        if f.is_finite() {
            out.push_str(ryu::Buffer::new().format(f));
        }
        return;
    }
    if val.is_sso() {
        let mut buf = [0u8; 5];
        let s = val.sso_as_str(&mut buf);
        write_csv_cell(out, s, delimiter);
        return;
    }
    if val.is_heap() {
        if let Some(HeapObj::Str(h)) = ctx.heap.get(val.as_heap_idx()) {
            write_csv_cell(out, h.as_str(), delimiter);
        }
    }
}

fn write_csv_cell(out: &mut String, s: &str, delimiter: char) {
    let needs_quotes =
        s.contains(delimiter) || s.contains('"') || s.contains('\n') || s.contains('\r');

    if !needs_quotes {
        out.push_str(s);
        return;
    }

    out.push('"');
    for c in s.chars() {
        if c == '"' {
            out.push_str("\"\"");
        } else {
            out.push(c);
        }
    }
    out.push('"');
}
