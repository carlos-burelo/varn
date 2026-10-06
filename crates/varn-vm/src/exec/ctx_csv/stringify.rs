use crate::exec::ExecCtx;
use crate::heap::HeapObj;
use varn_types::VmValue;

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

    match ctx.heap.get(value.as_heap()) {
        Some(HeapObj::Array(arr)) => {
            let repr = arr.repr();
            let items = match repr {
                varn_types::ArrayRepr::Boxed(v) => v.as_vec(),
                _ => return Err("CSV stringify expects a boxed array".to_string()),
            };

            if items.is_empty() {
                return Ok(String::new());
            }

            let first = items[0];
            if !first.is_heap() {
                return Err("CSV items must be objects or array rows".to_string());
            }

            match ctx.heap.get(first.as_heap()) {
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

                    for (i, name) in prop_names.iter().enumerate() {
                        if i > 0 {
                            out.push(delim_char);
                        }
                        write_csv_cell(&mut out, name, delim_char);
                    }
                    out.push('\n');

                    for item in items {
                        if !item.is_heap() {
                            continue;
                        }
                        if let Some(HeapObj::Object(obj) | HeapObj::Record(obj)) =
                            ctx.heap.get(item.as_heap())
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
                Some(HeapObj::Map(first_map)) => {
                    let map_b = first_map.borrow();
                    let prop_names: Vec<(varn_types::value::MapKey, String)> = map_b
                        .keys()
                        .map(|k| {
                            let name = ctx.heap.str_repr(k.0);
                            (*k, name)
                        })
                        .collect();
                    drop(map_b);

                    for (i, (_, name)) in prop_names.iter().enumerate() {
                        if i > 0 {
                            out.push(delim_char);
                        }
                        write_csv_cell(&mut out, name, delim_char);
                    }
                    out.push('\n');

                    for item in items {
                        if !item.is_heap() {
                            continue;
                        }
                        if let Some(HeapObj::Map(map)) = ctx.heap.get(item.as_heap()) {
                            let m = map.borrow();
                            for (slot, (k, _)) in prop_names.iter().enumerate() {
                                if slot > 0 {
                                    out.push(delim_char);
                                }
                                let val = m.get(k).copied().unwrap_or_else(VmValue::null);
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
                        if let Some(HeapObj::Array(row_arr)) = ctx.heap.get(item.as_heap()) {
                            match row_arr.repr() {
                                varn_types::ArrayRepr::Boxed(row_items) => {
                                    let row_items = row_items.as_vec();
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
        if let Some(HeapObj::Str(h)) = ctx.heap.get(val.as_heap()) {
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
