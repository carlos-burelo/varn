use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicI64, Ordering};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

static NEXT_FD: AtomicI64 = AtomicI64::new(1);

thread_local! {
    static FILES: RefCell<HashMap<i64, fs::File>> = RefCell::new(HashMap::new());
}

pub struct FsRuntime;

varn_contract! {
    module: "runtime:fs",
    contract: "src/modules/host/fs/fs_runtime.vn",
    impl FsRuntime {
        fn open(ctx: &mut dyn NativeCtx, path: &str, mode: &str) -> Result<i64, String> {
            use std::fs::OpenOptions;
            if mode == "r" {
                if !ctx.check_fs_read(path) {
                    return Err(format!("SecurityError: Permission denied (fs.read) for path '{path}'"));
                }
            } else {
                if !ctx.check_fs_write(path) {
                    return Err(format!("SecurityError: Permission denied (fs.write) for path '{path}'"));
                }
            }

            let file = match mode {
                "r" => OpenOptions::new().read(true).open(path),
                "w" => OpenOptions::new().write(true).create(true).truncate(true).open(path),
                "a" => OpenOptions::new().write(true).create(true).append(true).open(path),
                _ => OpenOptions::new().read(true).open(path),
            }.map_err(|e| e.to_string())?;

            let fd = NEXT_FD.fetch_add(1, Ordering::Relaxed);
            FILES.with(|files| {
                files.borrow_mut().insert(fd, file);
            });
            Ok(fd)
        }

        fn readFd(_ctx: &mut dyn NativeCtx, fd: i64, len: i64) -> Result<String, String> {
            FILES.with(|files| {
                let mut files_map = files.borrow_mut();
                let file = files_map.get_mut(&fd).ok_or_else(|| format!("invalid file descriptor {fd}"))?;

                use std::io::Read;
                let mut buf = vec![0u8; len as usize];
                let bytes_read = file.read(&mut buf).map_err(|e| e.to_string())?;
                buf.truncate(bytes_read);
                String::from_utf8(buf).map_err(|e| e.to_string())
            })
        }

        fn writeFd(_ctx: &mut dyn NativeCtx, fd: i64, data: &str) -> Result<i64, String> {
            FILES.with(|files| {
                let mut files_map = files.borrow_mut();
                let file = files_map.get_mut(&fd).ok_or_else(|| format!("invalid file descriptor {fd}"))?;

                use std::io::Write;
                file.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
                Ok(data.as_bytes().len() as i64)
            })
        }

        fn seek(_ctx: &mut dyn NativeCtx, fd: i64, offset: i64, whence: i64) -> Result<i64, String> {
            FILES.with(|files| {
                let mut files_map = files.borrow_mut();
                let file = files_map.get_mut(&fd).ok_or_else(|| format!("invalid file descriptor {fd}"))?;

                use std::io::{Seek, SeekFrom};
                let seek_from = match whence {
                    0 => SeekFrom::Start(offset as u64),
                    1 => SeekFrom::Current(offset),
                    2 => SeekFrom::End(offset),
                    _ => return Err(format!("invalid seek whence {whence}")),
                };
                let pos = file.seek(seek_from).map_err(|e| e.to_string())?;
                Ok(pos as i64)
            })
        }

        fn close(_ctx: &mut dyn NativeCtx, fd: i64) -> Result<(), String> {
            FILES.with(|files| {
                let mut files_map = files.borrow_mut();
                if files_map.remove(&fd).is_some() {
                    Ok(())
                } else {
                    Err(format!("invalid file descriptor {fd}"))
                }
            })
        }

        fn exists(ctx: &mut dyn NativeCtx, path: &str) -> Result<bool, String> {
            if !ctx.check_fs_read(path) {
                return Ok(false);
            }
            Ok(std::path::Path::new(path).exists())
        }

        fn stat(ctx: &mut dyn NativeCtx, path: &str) -> Result<VmValue, String> {
            if !ctx.check_fs_read(path) {
                return Err(format!("SecurityError: Permission denied (fs.read) for path '{path}'"));
            }
            let meta = fs::metadata(path).map_err(|e| e.to_string())?;
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let obj = ctx.alloc_object();
            let size_nv = VmValue::from_int(meta.len() as i64);
            let is_dir_nv = VmValue::from_bool(meta.is_dir());
            let is_file_nv = VmValue::from_bool(meta.is_file());
            let mtime_nv = VmValue::from_int(mtime);
            ctx.set_field(obj, "size", size_nv);
            ctx.set_field(obj, "isDir", is_dir_nv);
            ctx.set_field(obj, "isFile", is_file_nv);
            ctx.set_field(obj, "mtime", mtime_nv);
            Ok(obj)
        }

        fn mkdir(ctx: &mut dyn NativeCtx, path: &str) -> Result<(), String> {
            if !ctx.check_fs_write(path) {
                return Err(format!("SecurityError: Permission denied (fs.write) for path '{path}'"));
            }
            fs::create_dir(path).map_err(|e| e.to_string())
        }

        fn mkdirAll(ctx: &mut dyn NativeCtx, path: &str) -> Result<(), String> {
            if !ctx.check_fs_write(path) {
                return Err(format!("SecurityError: Permission denied (fs.write) for path '{path}'"));
            }
            fs::create_dir_all(path).map_err(|e| e.to_string())
        }

        fn readDir(ctx: &mut dyn NativeCtx, path: &str) -> Result<Vec<VmValue>, String> {
            if !ctx.check_fs_read(path) {
                return Err(format!("SecurityError: Permission denied (fs.read) for path '{path}'"));
            }
            let entries = fs::read_dir(path).map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    out.push(ctx.alloc_str(name));
                }
            }
            Ok(out)
        }

        fn remove(ctx: &mut dyn NativeCtx, path: &str) -> Result<(), String> {
            if !ctx.check_fs_write(path) {
                return Err(format!("SecurityError: Permission denied (fs.write) for path '{path}'"));
            }
            let p = std::path::Path::new(path);
            if p.is_dir() {
                fs::remove_dir(p).map_err(|e| e.to_string())
            } else {
                fs::remove_file(p).map_err(|e| e.to_string())
            }
        }

        fn removeAll(ctx: &mut dyn NativeCtx, path: &str) -> Result<(), String> {
            if !ctx.check_fs_write(path) {
                return Err(format!("SecurityError: Permission denied (fs.write) for path '{path}'"));
            }
            let p = std::path::Path::new(path);
            if p.is_dir() {
                fs::remove_dir_all(p).map_err(|e| e.to_string())
            } else {
                fs::remove_file(p).map_err(|e| e.to_string())
            }
        }

        fn rename(ctx: &mut dyn NativeCtx, from: &str, to: &str) -> Result<(), String> {
            if !ctx.check_fs_read(from) || !ctx.check_fs_write(to) {
                return Err(format!("SecurityError: Permission denied (fs.write) for path '{to}'"));
            }
            fs::rename(from, to).map_err(|e| e.to_string())
        }

        fn tempDir(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(std::env::temp_dir().to_string_lossy().into_owned())
        }
    }
}
