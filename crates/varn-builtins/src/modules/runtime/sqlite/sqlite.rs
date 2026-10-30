use rusqlite::Connection;
use std::time::Duration;
use varn_macros::varn_contract;
use varn_types::value::SendValue;
use varn_types::{NativeCtx, NativeError, VmValue, VnArray};

use crate::runtime::blocking;

#[path = "handles.rs"]
mod handles;
#[path = "rows.rs"]
mod rows;
use handles::{failure, lookup, register, release};

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const MEMORY: &str = ":memory:";

pub struct SqliteRuntime;

fn open_connection(path: &str) -> Result<Connection, NativeError> {
    let conn = if path == MEMORY {
        Connection::open_in_memory()
    } else {
        Connection::open(path)
    }
    .map_err(|e| failure(e, path))?;
    conn.busy_timeout(BUSY_TIMEOUT).map_err(|e| failure(e, path))?;
    conn.pragma_update(None, "foreign_keys", true)
        .map_err(|e| failure(e, path))?;
    Ok(conn)
}

fn allow_path(ctx: &mut dyn NativeCtx, path: &str) -> Result<(), NativeError> {
    if path == MEMORY {
        return Ok(());
    }
    if !ctx.check_fs_read(path) {
        return Err(NativeError::capability("fs.read", path));
    }
    if !ctx.check_fs_write(path) {
        return Err(NativeError::capability("fs.write", path));
    }
    Ok(())
}

varn_contract! {
    module: "runtime:sqlite",
    contract: "src/modules/runtime/sqlite/sqlite_runtime.vn",
    impl SqliteRuntime {
        fn open(ctx: &mut dyn NativeCtx, path: &str) -> Result<i64, NativeError> {
            allow_path(ctx, path)?;
            Ok(register(open_connection(path)?))
        }

        fn close(_ctx: &mut dyn NativeCtx, db_id: i64) -> Result<(), NativeError> {
            release(db_id);
            Ok(())
        }

        fn script(_ctx: &mut dyn NativeCtx, db_id: i64, sql: &str) -> Result<(), NativeError> {
            lookup(db_id)?.lock().execute_batch(sql).map_err(|e| failure(e, sql))
        }

        fn execute(ctx: &mut dyn NativeCtx, db_id: i64, sql: &str, params: VnArray) -> Result<i64, NativeError> {
            let bound = rows::params(ctx, params)?;
            rows::execute(&lookup(db_id)?.lock(), sql, bound)
        }

        fn executeAsync(ctx: &mut dyn NativeCtx, db_id: i64, sql: &str, params: VnArray) -> Result<VmValue, NativeError> {
            let bound = rows::params(ctx, params)?;
            let db = lookup(db_id)?;
            let sql = sql.to_owned();
            Ok(blocking::task(ctx, move || rows::execute(&db.lock(), &sql, bound).map(SendValue::Int)))
        }

        fn query(ctx: &mut dyn NativeCtx, db_id: i64, sql: &str, params: VnArray, limit: i64) -> Result<Vec<VmValue>, NativeError> {
            let bound = rows::params(ctx, params)?;
            let found = rows::select(&lookup(db_id)?.lock(), sql, bound, limit)?;
            Ok(found.iter().map(|row| row.to_value_ctx(ctx)).collect())
        }

        fn queryAsync(ctx: &mut dyn NativeCtx, db_id: i64, sql: &str, params: VnArray, limit: i64) -> Result<VmValue, NativeError> {
            let bound = rows::params(ctx, params)?;
            let db = lookup(db_id)?;
            let sql = sql.to_owned();
            Ok(blocking::task(ctx, move || {
                rows::select(&db.lock(), &sql, bound, limit).map(SendValue::Array)
            }))
        }
    }
}
