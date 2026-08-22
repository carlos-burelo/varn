use parking_lot::Mutex;
use rusqlite::types::ValueRef;
use rusqlite::Connection;
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue, VnArray};

static DB_POOL: Mutex<Vec<Option<Connection>>> = Mutex::new(Vec::new());

fn vm_params_to_sqlite(ctx: &dyn NativeCtx, params: VnArray) -> Vec<rusqlite::types::Value> {
    if params.raw().is_null() {
        return Vec::new();
    }
    let len = params.len(ctx);
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let v = params.get(ctx, i).unwrap_or_else(VmValue::null);
        if v.is_null() {
            out.push(rusqlite::types::Value::Null);
        } else if v.is_bool() {
            out.push(rusqlite::types::Value::Integer(if v.as_bool() { 1 } else { 0 }));
        } else if v.is_int() {
            out.push(rusqlite::types::Value::Integer(v.as_int()));
        } else if v.is_f64() {
            out.push(rusqlite::types::Value::Real(v.as_f64()));
        } else if let Some(s) = ctx.str_owned(v) {
            out.push(rusqlite::types::Value::Text(s));
        } else {
            out.push(rusqlite::types::Value::Null);
        }
    }
    out
}

pub struct SqliteRuntime;

varn_contract! {
    module: "runtime:sqlite",
    contract: "src/modules/host/sqlite/sqlite_runtime.vn",
    impl SqliteRuntime {
        fn open(_ctx: &mut dyn NativeCtx, path: &str) -> Result<i64, String> {
            let conn = if path == ":memory:" || path.is_empty() {
                Connection::open_in_memory()
            } else {
                Connection::open(path)
            }.map_err(|e| format!("SQLite open error: {e}"))?;

            let mut pool = DB_POOL.lock();
            let id = pool.len() as i64;
            pool.push(Some(conn));
            Ok(id)
        }

        fn close(_ctx: &mut dyn NativeCtx, db_id: i64) -> Result<bool, String> {
            if db_id < 0 {
                return Ok(false);
            }
            let mut pool = DB_POOL.lock();
            if let Some(slot) = pool.get_mut(db_id as usize) {
                if let Some(conn) = slot.take() {
                    let _ = conn.close();
                    return Ok(true);
                }
            }
            Ok(false)
        }

        fn exec(_ctx: &mut dyn NativeCtx, db_id: i64, sql: &str) -> Result<i64, String> {
            let pool = DB_POOL.lock();
            let conn = pool.get(db_id as usize)
                .and_then(|s| s.as_ref())
                .ok_or_else(|| format!("SQLite DB handle {db_id} not found"))?;

            let affected = conn.execute(sql, []).map_err(|e| format!("SQLite exec error: {e}"))?;
            Ok(affected as i64)
        }

        fn queryAll(ctx: &mut dyn NativeCtx, db_id: i64, sql: &str, params: VnArray) -> Result<Vec<VmValue>, String> {
            let pool = DB_POOL.lock();
            let conn = pool.get(db_id as usize)
                .and_then(|s| s.as_ref())
                .ok_or_else(|| format!("SQLite DB handle {db_id} not found"))?;

            let sql_params = vm_params_to_sqlite(ctx, params);
            let mut stmt = conn.prepare(sql).map_err(|e| format!("SQLite prepare error: {e}"))?;
            let col_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

            let mut rows = stmt.query(rusqlite::params_from_iter(sql_params))
                .map_err(|e| format!("SQLite query error: {e}"))?;

            let mut out = Vec::new();
            while let Some(row) = rows.next().map_err(|e| format!("SQLite row error: {e}"))? {
                let obj = ctx.alloc_object();
                for (i, name) in col_names.iter().enumerate() {
                    let val_ref = row.get_ref(i).map_err(|e| format!("SQLite column error: {e}"))?;
                    let vm_val = match val_ref {
                        ValueRef::Null => VmValue::null(),
                        ValueRef::Integer(v) => VmValue::from_int(v),
                        ValueRef::Real(v) => VmValue::from_f64(v),
                        ValueRef::Text(v) => {
                            let s = String::from_utf8_lossy(v);
                            ctx.alloc_str(s.as_ref())
                        }
                        ValueRef::Blob(v) => {
                            let s = hex::encode(v);
                            ctx.alloc_str_owned(s)
                        }
                    };
                    ctx.set_field(obj, name, vm_val);
                }
                out.push(obj);
            }

            Ok(out)
        }

        fn queryOne(ctx: &mut dyn NativeCtx, db_id: i64, sql: &str, params: VnArray) -> Result<VmValue, String> {
            let all = Self::queryAll(ctx, db_id, sql, params)?;
            Ok(all.into_iter().next().unwrap_or_else(VmValue::null))
        }
    }
}
