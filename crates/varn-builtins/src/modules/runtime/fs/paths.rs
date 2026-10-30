use super::descriptors::fs_error;
use std::fs;
use std::io::Write;
use std::path::Path;
use varn_types::NativeError;

pub fn read_text(path: &str) -> Result<String, NativeError> {
    fs::read_to_string(path).map_err(|e| fs_error(e, path))
}

pub fn read_bytes(path: &str) -> Result<Vec<u8>, NativeError> {
    fs::read(path).map_err(|e| fs_error(e, path))
}

pub fn read_lines(path: &str) -> Result<Vec<String>, NativeError> {
    Ok(read_text(path)?.lines().map(str::to_owned).collect())
}

pub fn read_dir(path: &str) -> Result<Vec<String>, NativeError> {
    Ok(fs::read_dir(path)
        .map_err(|e| fs_error(e, path))?
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .collect())
}

pub fn write(path: &str, data: &[u8]) -> Result<(), NativeError> {
    fs::write(path, data).map_err(|e| fs_error(e, path))
}

pub fn append(path: &str, data: &[u8]) -> Result<(), NativeError> {
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| fs_error(e, path))?
        .write_all(data)
        .map_err(|e| fs_error(e, path))
}

pub fn copy(from: &str, to: &str) -> Result<(), NativeError> {
    fs::copy(from, to)
        .map(drop)
        .map_err(|e| fs_error(e, from))
}

pub fn remove_all(path: &str) -> Result<(), NativeError> {
    let target = Path::new(path);
    if target.is_dir() {
        fs::remove_dir_all(target).map_err(|e| fs_error(e, path))
    } else {
        fs::remove_file(target).map_err(|e| fs_error(e, path))
    }
}
