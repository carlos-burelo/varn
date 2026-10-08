use rustc_hash::FxHashMap;
use std::fs;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

static NEXT_FD: AtomicI64 = AtomicI64::new(1);
static FILES: LazyLock<RwLock<FxHashMap<i64, Arc<Mutex<fs::File>>>>> =
    LazyLock::new(|| RwLock::new(FxHashMap::default()));

fn coded(code: &str, msg: impl std::fmt::Display) -> String {
    format!("{code}|{msg}")
}

fn fs_io_err(e: std::io::Error) -> String {
    let code = match e.kind() {
        std::io::ErrorKind::NotFound => "E_FS_NOT_FOUND",
        std::io::ErrorKind::PermissionDenied => "E_FS_DENIED",
        std::io::ErrorKind::AlreadyExists => "E_FS_EXISTS",
        _ => "E_FS_IO",
    };
    coded(code, e)
}

fn denied(action: &str, path: &str) -> String {
    coded(
        "E_FS_DENIED",
        format!("SecurityError: Permission denied ({action}) for path '{path}'"),
    )
}

pub struct FsRuntime;

varn_contract! {
    module: "runtime:fs",
    contract: "src/modules/runtime/fs/fs_runtime.vn",
    impl FsRuntime {
        fn open(ctx: &mut dyn NativeCtx, path: &str, mode: &str) -> Result<i64, String> {
            use std::fs::OpenOptions;
            if mode == "r" {
                if !ctx.check_fs_read(path) {
                    return Err(denied("fs.read", path));
                }
            } else {
                if !ctx.check_fs_write(path) {
                    return Err(denied("fs.write", path));
                }
            }

            let file = match mode {
                "r" => OpenOptions::new().read(true).open(path),
                "w" => OpenOptions::new().write(true).create(true).truncate(true).open(path),
                "a" => OpenOptions::new().create(true).append(true).open(path),
                _ => OpenOptions::new().read(true).open(path),
            }.map_err(fs_io_err)?;

            let fd = NEXT_FD.fetch_add(1, Ordering::Relaxed);
            FILES.write().unwrap().insert(fd, Arc::new(Mutex::new(file)));
            Ok(fd)
        }

        fn readFd(_ctx: &mut dyn NativeCtx, fd: i64, len: i64) -> Result<String, String> {
            let file_arc = {
                let map = FILES.read().unwrap();
                map.get(&fd).cloned().ok_or_else(|| coded("E_FS_BAD_FD", format!("invalid file descriptor {fd}")))?
            };
            let mut file = file_arc.lock().unwrap();

            use std::io::Read;
            let mut buf = vec![0u8; len as usize];
            let bytes_read = file.read(&mut buf).map_err(fs_io_err)?;
            buf.truncate(bytes_read);
            String::from_utf8(buf).map_err(|e| coded("E_FS_ENCODING", e))
        }

        fn readFdBytes(ctx: &mut dyn NativeCtx, fd: i64, len: i64) -> Result<VmValue, String> {
            let file_arc = {
                let map = FILES.read().unwrap();
                map.get(&fd).cloned().ok_or_else(|| coded("E_FS_BAD_FD", format!("invalid file descriptor {fd}")))?
            };
            let mut file = file_arc.lock().unwrap();

            use std::io::Read;
            let mut buf = vec![0u8; len as usize];
            let bytes_read = file.read(&mut buf).map_err(fs_io_err)?;
            buf.truncate(bytes_read);
            Ok(ctx.alloc_buffer_from_bytes(&buf))
        }

        fn writeFd(_ctx: &mut dyn NativeCtx, fd: i64, data: &str) -> Result<i64, String> {
            let file_arc = {
                let map = FILES.read().unwrap();
                map.get(&fd).cloned().ok_or_else(|| coded("E_FS_BAD_FD", format!("invalid file descriptor {fd}")))?
            };
            let mut file = file_arc.lock().unwrap();

            use std::io::Write;
            file.write_all(data.as_bytes()).map_err(fs_io_err)?;
            Ok(data.len() as i64)
        }

        fn writeFdBytes(ctx: &mut dyn NativeCtx, fd: i64, data: VmValue) -> Result<i64, String> {
            let file_arc = {
                let map = FILES.read().unwrap();
                map.get(&fd).cloned().ok_or_else(|| coded("E_FS_BAD_FD", format!("invalid file descriptor {fd}")))?
            };
            let mut file = file_arc.lock().unwrap();

            let bytes = ctx.buffer_to_bytes(data).ok_or_else(|| coded("E_FS_BAD_ARG", "expected Bytes"))?;
            use std::io::Write;
            file.write_all(&bytes).map_err(fs_io_err)?;
            Ok(bytes.len() as i64)
        }

        fn readFileBytes(ctx: &mut dyn NativeCtx, path: &str) -> Result<VmValue, String> {
            if !ctx.check_fs_read(path) {
                return Err(denied("fs.read", path));
            }
            let bytes = fs::read(path).map_err(fs_io_err)?;
            Ok(ctx.alloc_buffer_from_bytes(&bytes))
        }

        fn writeFileBytes(ctx: &mut dyn NativeCtx, path: &str, data: VmValue) -> Result<(), String> {
            if !ctx.check_fs_write(path) {
                return Err(denied("fs.write", path));
            }
            let bytes = ctx.buffer_to_bytes(data).ok_or_else(|| coded("E_FS_BAD_ARG", "expected Bytes"))?;
            fs::write(path, bytes).map_err(fs_io_err)
        }

        fn seek(_ctx: &mut dyn NativeCtx, fd: i64, offset: i64, whence: i64) -> Result<i64, String> {
            let file_arc = {
                let map = FILES.read().unwrap();
                map.get(&fd).cloned().ok_or_else(|| coded("E_FS_BAD_FD", format!("invalid file descriptor {fd}")))?
            };
            let mut file = file_arc.lock().unwrap();

            use std::io::{Seek, SeekFrom};
            let seek_from = match whence {
                0 => SeekFrom::Start(offset as u64),
                1 => SeekFrom::Current(offset),
                2 => SeekFrom::End(offset),
                _ => return Err(coded("E_FS_BAD_ARG", format!("invalid seek whence {whence}"))),
            };
            let pos = file.seek(seek_from).map_err(fs_io_err)?;
            Ok(pos as i64)
        }

        fn close(_ctx: &mut dyn NativeCtx, fd: i64) -> Result<(), String> {
            if FILES.write().unwrap().remove(&fd).is_some() {
                Ok(())
            } else {
                Err(coded("E_FS_BAD_FD", format!("invalid file descriptor {fd}")))
            }
        }

        fn exists(ctx: &mut dyn NativeCtx, path: &str) -> Result<bool, String> {
            if !ctx.check_fs_read(path) {
                return Ok(false);
            }
            Ok(std::path::Path::new(path).exists())
        }

        fn stat(ctx: &mut dyn NativeCtx, path: &str) -> Result<VmValue, String> {
            if !ctx.check_fs_read(path) {
                return Err(denied("fs.read", path));
            }
            let meta = fs::metadata(path).map_err(fs_io_err)?;
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
                return Err(denied("fs.write", path));
            }
            fs::create_dir(path).map_err(fs_io_err)
        }

        fn mkdirAll(ctx: &mut dyn NativeCtx, path: &str) -> Result<(), String> {
            if !ctx.check_fs_write(path) {
                return Err(denied("fs.write", path));
            }
            fs::create_dir_all(path).map_err(fs_io_err)
        }

        fn readDir(ctx: &mut dyn NativeCtx, path: &str) -> Result<Vec<VmValue>, String> {
            if !ctx.check_fs_read(path) {
                return Err(denied("fs.read", path));
            }
            let entries = fs::read_dir(path).map_err(fs_io_err)?;
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
                return Err(denied("fs.write", path));
            }
            let p = std::path::Path::new(path);
            if p.is_dir() {
                fs::remove_dir(p).map_err(fs_io_err)
            } else {
                fs::remove_file(p).map_err(fs_io_err)
            }
        }

        fn removeAll(ctx: &mut dyn NativeCtx, path: &str) -> Result<(), String> {
            if !ctx.check_fs_write(path) {
                return Err(denied("fs.write", path));
            }
            let p = std::path::Path::new(path);
            if p.is_dir() {
                fs::remove_dir_all(p).map_err(fs_io_err)
            } else {
                fs::remove_file(p).map_err(fs_io_err)
            }
        }

        fn rename(ctx: &mut dyn NativeCtx, from: &str, to: &str) -> Result<(), String> {
            if !ctx.check_fs_read(from) || !ctx.check_fs_write(to) {
                return Err(denied("fs.write", to));
            }
            fs::rename(from, to).map_err(fs_io_err)
        }

        fn copyFile(ctx: &mut dyn NativeCtx, from: &str, to: &str) -> Result<(), String> {
            if !ctx.check_fs_read(from) || !ctx.check_fs_write(to) {
                return Err(denied("fs.write", to));
            }
            fs::copy(from, to).map(|_| ()).map_err(fs_io_err)
        }

        fn readFileText(ctx: &mut dyn NativeCtx, path: &str) -> Result<String, String> {
            if !ctx.check_fs_read(path) {
                return Err(denied("fs.read", path));
            }
            fs::read_to_string(path).map_err(fs_io_err)
        }

        fn writeFileText(ctx: &mut dyn NativeCtx, path: &str, data: &str) -> Result<(), String> {
            if !ctx.check_fs_write(path) {
                return Err(denied("fs.write", path));
            }
            fs::write(path, data.as_bytes()).map_err(fs_io_err)
        }

        fn appendFileText(ctx: &mut dyn NativeCtx, path: &str, data: &str) -> Result<(), String> {
            if !ctx.check_fs_write(path) {
                return Err(denied("fs.write", path));
            }
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(fs_io_err)?
                .write_all(data.as_bytes())
                .map_err(fs_io_err)
        }

        fn readLines(ctx: &mut dyn NativeCtx, path: &str) -> Result<Vec<VmValue>, String> {
            if !ctx.check_fs_read(path) {
                return Err(denied("fs.read", path));
            }
            let content = fs::read_to_string(path).map_err(fs_io_err)?;
            Ok(content.lines().map(|l| ctx.alloc_str(l)).collect())
        }

        fn readFdAll(_ctx: &mut dyn NativeCtx, fd: i64) -> Result<String, String> {
            let file_arc = {
                let map = FILES.read().unwrap();
                map.get(&fd).cloned().ok_or_else(|| coded("E_FS_BAD_FD", format!("invalid file descriptor {fd}")))?
            };
            let mut file = file_arc.lock().unwrap();
            use std::io::Read;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).map_err(fs_io_err)?;
            String::from_utf8(buf).map_err(|e| coded("E_FS_ENCODING", e))
        }

        fn readFdAllBytes(ctx: &mut dyn NativeCtx, fd: i64) -> Result<VmValue, String> {
            let file_arc = {
                let map = FILES.read().unwrap();
                map.get(&fd).cloned().ok_or_else(|| coded("E_FS_BAD_FD", format!("invalid file descriptor {fd}")))?
            };
            let mut file = file_arc.lock().unwrap();
            use std::io::Read;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).map_err(fs_io_err)?;
            Ok(ctx.alloc_buffer_from_bytes(&buf))
        }

        fn tempDir(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(std::env::temp_dir().to_string_lossy().into_owned())
        }
    }
}
