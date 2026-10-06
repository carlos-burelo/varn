use super::scan::FastCsvParser;
use crate::exec::ExecCtx;
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

    if let Ok(f) = s.parse::<f64>() {
        if f.is_finite() {
            return VmValue::from_f64(f);
        }
    }

    if let Some(sso) = VmValue::try_from_sso(s) {
        return sso;
    }

    ctx.heap.alloc_str_dynamic(s)
}
