use parking_lot::Mutex;
use rusqlite::{Connection, ErrorCode};
use std::sync::Arc;
use varn_core::{ErrorField, StdErrorClass};
use varn_types::NativeError;

pub type Shared = Arc<Mutex<Connection>>;

static POOL: Mutex<Vec<Option<Shared>>> = Mutex::new(Vec::new());

pub fn register(conn: Connection) -> i64 {
    let mut pool = POOL.lock();
    pool.push(Some(Arc::new(Mutex::new(conn))));
    pool.len() as i64 - 1
}

pub fn lookup(id: i64) -> Result<Shared, NativeError> {
    usize::try_from(id)
        .ok()
        .and_then(|slot| POOL.lock().get(slot).cloned().flatten())
        .ok_or_else(|| NativeError::std(StdErrorClass::Closed, format!("sqlite: database {id} is closed")))
}

pub fn release(id: i64) {
    if let Ok(slot) = usize::try_from(id)
        && let Some(entry) = POOL.lock().get_mut(slot)
    {
        entry.take();
    }
}

pub fn failure(error: rusqlite::Error, sql: &str) -> NativeError {
    let class = if error.sqlite_error_code() == Some(ErrorCode::ConstraintViolation) {
        StdErrorClass::SqlConstraint
    } else {
        StdErrorClass::Sql
    };
    NativeError::std(class, error.to_string()).with_str(ErrorField::Sql, sql)
}
