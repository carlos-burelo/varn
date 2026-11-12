use super::scan::FastCsvParser;
use crate::exec::ExecCtx;
use vn_types::value::root_shape;
use vn_types::VmValue;

pub(crate) fn parse_csv(
    ctx: &mut ExecCtx,
    text: &str,
    delimiter: u8,
    has_header: bool,
    trim: bool,
) -> Result<VmValue, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut parser = FastCsvParser::new(text.as_bytes(), delimiter, trim);
    let mut row_buf: Vec<std::borrow::Cow<'_, str>> = Vec::with_capacity(32);

    if !parser.next_row_into(&mut row_buf)? || row_buf.is_empty() {
        return Ok(ctx.heap.alloc_array_vm(Vec::new()));
    }

    if has_header {
        let mut shape = root_shape();
        for (index, col_name) in row_buf.iter().enumerate() {
            if row_buf[..index].iter().any(|earlier| earlier == col_name) {
                return Err(format!("duplicate column '{col_name}' in the CSV header"));
            }
            shape = shape.transition(col_name.as_ref().into());
        }
        let num_cols = row_buf.len();
        let mut row_number = 1usize;

        let est_rows = (text.len() / (num_cols * 8).max(16)).clamp(16, 131072);
        let mut out_objects: Vec<VmValue> = Vec::with_capacity(est_rows);
        let mut field_values: Vec<VmValue> = Vec::with_capacity(num_cols);

        while parser.next_row_into(&mut row_buf)? {
            if row_buf.is_empty() || (row_buf.len() == 1 && row_buf[0].is_empty()) {
                continue;
            }
            row_number += 1;
            if row_buf.len() != num_cols {
                return Err(format!(
                    "CSV row {row_number} has {} fields but the header has {num_cols}",
                    row_buf.len()
                ));
            }
            field_values.clear();
            for cell in &row_buf {
                field_values.push(parse_cell_value(ctx, cell.as_ref()));
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

    if let Some(number) = canonical_number(s) {
        return match number {
            Number::Int(value) => VmValue::from_int(value),
            Number::Float(value) => VmValue::from_f64(value),
        };
    }

    if let Some(sso) = VmValue::try_from_sso(s) {
        return sso;
    }

    ctx.heap.alloc_str_dynamic(s)
}

enum Number {
    Int(i64),
    Float(f64),
}

fn canonical_number(s: &str) -> Option<Number> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    let whole_len = digits.find(['.', 'e', 'E']).unwrap_or(digits.len());
    let whole = &digits[..whole_len];
    let plain_whole = whole == "0"
        || (!whole.is_empty()
            && !whole.starts_with('0')
            && whole.bytes().all(|b| b.is_ascii_digit()));
    if !plain_whole {
        return None;
    }
    if whole_len == digits.len() {
        return s.parse::<i64>().ok().map(Number::Int);
    }
    let rest = &digits[whole_len..];
    let well_formed = match rest.strip_prefix('.') {
        Some(tail) => {
            let (fraction, exponent) = tail.split_once(['e', 'E']).unwrap_or((tail, ""));
            !fraction.is_empty()
                && fraction.bytes().all(|b| b.is_ascii_digit())
                && (exponent.is_empty() || exponent_ok(exponent))
        }
        None => rest.strip_prefix(['e', 'E']).is_some_and(exponent_ok),
    };
    if !well_formed {
        return None;
    }
    s.parse::<f64>()
        .ok()
        .filter(|f| f.is_finite())
        .map(Number::Float)
}

fn exponent_ok(text: &str) -> bool {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}
