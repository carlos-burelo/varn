use rustc_hash::FxHashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use varn_core::{ErrorField, RuntimeErrorKind, StdErrorClass};
use varn_types::NativeError;

const MAX_CHUNK: usize = 64 << 20;

pub type Handle = Arc<Mutex<File>>;

static NEXT_FD: AtomicI64 = AtomicI64::new(1);
static FILES: LazyLock<RwLock<FxHashMap<i64, Handle>>> =
    LazyLock::new(|| RwLock::new(FxHashMap::default()));

pub fn io_error(e: std::io::Error) -> NativeError {
    NativeError::std(StdErrorClass::Io, e.to_string())
}

pub fn fs_error(e: std::io::Error, path: &str) -> NativeError {
    let kind = e.kind();
    let class = if kind == std::io::ErrorKind::NotFound {
        StdErrorClass::FsNotFound
    } else if kind == std::io::ErrorKind::PermissionDenied {
        StdErrorClass::FsPermission
    } else if kind == std::io::ErrorKind::AlreadyExists {
        StdErrorClass::FsExists
    } else {
        StdErrorClass::Fs
    };
    NativeError::std(class, format!("{path}: {e}")).with_str(ErrorField::Path, path)
}

pub fn encoding_error(e: std::string::FromUtf8Error) -> NativeError {
    NativeError::std(StdErrorClass::FsEncoding, e.to_string())
}

pub fn type_error(message: &str) -> NativeError {
    NativeError::rethrow(RuntimeErrorKind::TypeError, message, None)
}

fn bad_fd(fd: i64) -> NativeError {
    NativeError::std(
        StdErrorClass::BadDescriptor,
        format!("invalid file descriptor {fd}"),
    )
}

pub fn register(file: File) -> i64 {
    let fd = NEXT_FD.fetch_add(1, Ordering::Relaxed);
    FILES
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .insert(fd, Arc::new(Mutex::new(file)));
    fd
}

pub fn handle(fd: i64) -> Result<Handle, NativeError> {
    FILES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(&fd)
        .cloned()
        .ok_or_else(|| bad_fd(fd))
}

pub fn unregister(fd: i64) -> Result<(), NativeError> {
    FILES
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&fd)
        .map(drop)
        .ok_or_else(|| bad_fd(fd))
}

pub fn read_some(handle: &Handle, len: i64) -> Result<Vec<u8>, NativeError> {
    let len = usize::try_from(len)
        .map_err(|_| {
            NativeError::rethrow(
                RuntimeErrorKind::RangeError,
                "read length must not be negative",
                None,
            )
        })?
        .min(MAX_CHUNK);
    let mut buf = vec![0u8; len];
    let read = handle
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .read(&mut buf)
        .map_err(io_error)?;
    buf.truncate(read);
    Ok(buf)
}

pub fn read_all(handle: &Handle) -> Result<Vec<u8>, NativeError> {
    let mut buf = Vec::new();
    handle
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .read_to_end(&mut buf)
        .map_err(io_error)?;
    Ok(buf)
}

pub fn write_all(handle: &Handle, data: &[u8]) -> Result<(), NativeError> {
    handle
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .write_all(data)
        .map_err(io_error)
}
