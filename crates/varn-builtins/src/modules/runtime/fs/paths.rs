use super::descriptors::fs_io_err;
use std::fs;
use std::io::Write;
use std::path::Path;

pub fn read_text(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(fs_io_err)
}

pub fn read_bytes(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(fs_io_err)
}

pub fn read_lines(path: &str) -> Result<Vec<String>, String> {
    Ok(read_text(path)?.lines().map(str::to_owned).collect())
}

pub fn read_dir(path: &str) -> Result<Vec<String>, String> {
    Ok(fs::read_dir(path)
        .map_err(fs_io_err)?
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .collect())
}

pub fn write(path: &str, data: &[u8]) -> Result<(), String> {
    fs::write(path, data).map_err(fs_io_err)
}

pub fn append(path: &str, data: &[u8]) -> Result<(), String> {
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(fs_io_err)?
        .write_all(data)
        .map_err(fs_io_err)
}

pub fn copy(from: &str, to: &str) -> Result<(), String> {
    fs::copy(from, to).map(drop).map_err(fs_io_err)
}

pub fn remove_all(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(fs_io_err)
    } else {
        fs::remove_file(path).map_err(fs_io_err)
    }
}
