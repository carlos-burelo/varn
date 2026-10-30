use rustc_hash::FxHashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};

const MAX_CHUNK: usize = 64 << 20;

pub type Handle = Arc<Mutex<File>>;

static NEXT_FD: AtomicI64 = AtomicI64::new(1);
static FILES: LazyLock<RwLock<FxHashMap<i64, Handle>>> =
    LazyLock::new(|| RwLock::new(FxHashMap::default()));

pub fn coded(code: &str, msg: impl std::fmt::Display) -> String {
    format!("{code}|{msg}")
}

pub fn fs_io_err(e: std::io::Error) -> String {
    let kind = e.kind();
    let code = if kind == std::io::ErrorKind::NotFound {
        "E_FS_NOT_FOUND"
    } else if kind == std::io::ErrorKind::PermissionDenied {
        "E_FS_DENIED"
    } else if kind == std::io::ErrorKind::AlreadyExists {
        "E_FS_EXISTS"
    } else {
        "E_FS_IO"
    };
    coded(code, e)
}

fn bad_fd(fd: i64) -> String {
    coded("E_FS_BAD_FD", format!("invalid file descriptor {fd}"))
}

pub fn register(file: File) -> i64 {
    let fd = NEXT_FD.fetch_add(1, Ordering::Relaxed);
    FILES
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .insert(fd, Arc::new(Mutex::new(file)));
    fd
}

pub fn handle(fd: i64) -> Result<Handle, String> {
    FILES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(&fd)
        .cloned()
        .ok_or_else(|| bad_fd(fd))
}

pub fn unregister(fd: i64) -> Result<(), String> {
    FILES
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&fd)
        .map(drop)
        .ok_or_else(|| bad_fd(fd))
}

pub fn read_some(handle: &Handle, len: i64) -> Result<Vec<u8>, String> {
    let len = usize::try_from(len)
        .map_err(|_| coded("E_FS_BAD_ARG", "read length must not be negative"))?
        .min(MAX_CHUNK);
    let mut buf = vec![0u8; len];
    let read = handle
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .read(&mut buf)
        .map_err(fs_io_err)?;
    buf.truncate(read);
    Ok(buf)
}

pub fn read_all(handle: &Handle) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    handle
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .read_to_end(&mut buf)
        .map_err(fs_io_err)?;
    Ok(buf)
}

pub fn write_all(handle: &Handle, data: &[u8]) -> Result<(), String> {
    handle
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .write_all(data)
        .map_err(fs_io_err)
}
