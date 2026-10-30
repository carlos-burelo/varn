use rusqlite::types::{Value, ValueRef};
use rusqlite::Connection;
use varn_core::{ErrorField, StdErrorClass};
use varn_types::value::SendValue;
use varn_types::{NativeCtx, NativeError, VnArray};

use super::handles::failure;

pub fn params(ctx: &mut dyn NativeCtx, array: VnArray) -> Result<Vec<Value>, NativeError> {
    if array.raw().is_null() {
        return Ok(Vec::new());
    }
    (0..array.len(ctx))
        .map(|i| {
            let v = array.get(ctx, i).unwrap_or_else(|| ctx.null_val());
            if v.is_null() {
                Ok(Value::Null)
            } else if v.is_bool() {
                Ok(Value::Integer(i64::from(v.as_bool())))
            } else if v.is_int() {
                Ok(Value::Integer(v.as_int()))
            } else if v.is_f64() {
                Ok(Value::Real(v.as_f64()))
            } else if ctx.is_buffer(v) {
                ctx.buffer_to_bytes(v).map(Value::Blob).ok_or_else(|| {
                    NativeError::type_error(format!("sqlite: parameter {i} is an unreadable Bytes"))
                })
            } else if let Some(text) = ctx.str_owned(v) {
                Ok(Value::Text(text))
            } else {
                Err(NativeError::type_error(format!(
                    "sqlite: parameter {i} must be str, int, float, bool, Bytes or null"
                )))
            }
        })
        .collect()
}

fn column(sql: &str, name: &str, value: ValueRef<'_>) -> Result<SendValue, NativeError> {
    Ok(match value {
        ValueRef::Null => SendValue::Null,
        ValueRef::Integer(n) => SendValue::Int(n),
        ValueRef::Real(f) => SendValue::Float(f.to_bits()),
        ValueRef::Text(bytes) => SendValue::Str(
            std::str::from_utf8(bytes)
                .map_err(|_| {
                    NativeError::std(
                        StdErrorClass::Sql,
                        format!("sqlite: column '{name}' is not valid UTF-8"),
                    )
                    .with_str(ErrorField::Sql, sql)
                })?
                .to_owned(),
        ),
        ValueRef::Blob(bytes) => SendValue::Bytes(bytes.to_vec()),
    })
}

pub fn select(conn: &Connection, sql: &str, bound: Vec<Value>, limit: i64) -> Result<Vec<SendValue>, NativeError> {
    let mut stmt = conn.prepare(sql).map_err(|e| failure(e, sql))?;
    let names: Vec<String> = stmt.column_names().iter().map(|n| n.to_string()).collect();
    let mut cursor = stmt
        .query(rusqlite::params_from_iter(bound))
        .map_err(|e| failure(e, sql))?;
    let cap = usize::try_from(limit).unwrap_or(usize::MAX);
    let mut out = Vec::new();
    while out.len() < cap {
        let Some(row) = cursor.next().map_err(|e| failure(e, sql))? else {
            break;
        };
        let mut fields = Vec::with_capacity(names.len());
        for (i, name) in names.iter().enumerate() {
            let value = row.get_ref(i).map_err(|e| failure(e, sql))?;
            fields.push((name.clone(), column(sql, name, value)?));
        }
        out.push(SendValue::Object(fields));
    }
    Ok(out)
}

pub fn execute(conn: &Connection, sql: &str, bound: Vec<Value>) -> Result<i64, NativeError> {
    conn.execute(sql, rusqlite::params_from_iter(bound))
        .map(|changed| changed as i64)
        .map_err(|e| failure(e, sql))
}
