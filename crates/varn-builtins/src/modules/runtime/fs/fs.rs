use std::fs;
use varn_macros::varn_contract;
use varn_types::value::SendValue;
use varn_types::{NativeCtx, VmValue};

use crate::runtime::blocking;

#[path = "descriptors.rs"]
mod descriptors;
#[path = "paths.rs"]
mod paths;
use descriptors::{coded, fs_io_err};

fn allow_read(ctx: &mut dyn NativeCtx, path: &str) -> Result<(), String> {
    if ctx.check_fs_read(path) {
        Ok(())
    } else {
        Err(denied("fs.read", path))
    }
}

fn allow_write(ctx: &mut dyn NativeCtx, path: &str) -> Result<(), String> {
    if ctx.check_fs_write(path) {
        Ok(())
    } else {
        Err(denied("fs.write", path))
    }
}

fn allow_copy(ctx: &mut dyn NativeCtx, from: &str, to: &str) -> Result<(), String> {
    if ctx.check_fs_read(from) && ctx.check_fs_write(to) {
        Ok(())
    } else {
        Err(denied("fs.write", to))
    }
}

fn strings(ctx: &mut dyn NativeCtx, items: Vec<String>) -> Vec<VmValue> {
    items.iter().map(|item| ctx.alloc_str(item)).collect()
}

fn bytes_arg(ctx: &mut dyn NativeCtx, data: VmValue) -> Result<Vec<u8>, String> {
    ctx.buffer_to_bytes(data)
        .ok_or_else(|| coded("E_FS_BAD_ARG", "expected Bytes"))
}

fn text_array(items: Vec<String>) -> SendValue {
    SendValue::Array(items.into_iter().map(SendValue::Str).collect())
}

fn denied(action: &str, path: &str) -> String {
    coded(
        "E_FS_DENIED",
        format!("SecurityError: Permission denied ({action}) for path '{path}'"),
    )
}

pub struct FsRuntime;

varn_macros::varn_record! {
    contract: "src/modules/runtime/fs/fs_runtime.vn",
    interface: "RawStats",
    struct RawStats;
}

varn_macros::varn_enum! {
    module: MODULE_ID,
    contract: "src/modules/runtime/fs/fs_runtime.vn",
    pub enum FileMode;
}

varn_macros::varn_enum! {
    module: MODULE_ID,
    contract: "src/modules/runtime/fs/fs_runtime.vn",
    pub enum SeekOrigin;
}

varn_contract! {
    module: "runtime:fs",
    contract: "src/modules/runtime/fs/fs_runtime.vn",
    impl FsRuntime {
        fn open(ctx: &mut dyn NativeCtx, path: &str, mode: VmValue) -> Result<i64, String> {
            use std::fs::OpenOptions;
            let mode = FileMode::from_value(ctx, mode)
                .ok_or_else(|| coded("E_FS_BAD_ARG", "expected a FileMode"))?;
            let (allowed, capability) = match mode {
                FileMode::Read => (ctx.check_fs_read(path), "fs.read"),
                FileMode::Write | FileMode::Append => (ctx.check_fs_write(path), "fs.write"),
            };
            if !allowed {
                return Err(denied(capability, path));
            }

            let file = match mode {
                FileMode::Read => OpenOptions::new().read(true).open(path),
                FileMode::Write => OpenOptions::new().write(true).create(true).truncate(true).open(path),
                FileMode::Append => OpenOptions::new().create(true).append(true).open(path),
            }.map_err(fs_io_err)?;

            Ok(descriptors::register(file))
        }

        fn readFd(_ctx: &mut dyn NativeCtx, fd: i64, len: i64) -> Result<String, String> {
            let buf = descriptors::read_some(&descriptors::handle(fd)?, len)?;
            String::from_utf8(buf).map_err(|e| coded("E_FS_ENCODING", e))
        }

        fn readFdBytes(ctx: &mut dyn NativeCtx, fd: i64, len: i64) -> Result<VmValue, String> {
            let buf = descriptors::read_some(&descriptors::handle(fd)?, len)?;
            Ok(ctx.alloc_buffer_from_bytes(&buf))
        }

        fn writeFd(_ctx: &mut dyn NativeCtx, fd: i64, data: &str) -> Result<i64, String> {
            descriptors::write_all(&descriptors::handle(fd)?, data.as_bytes())?;
            Ok(data.len() as i64)
        }

        fn writeFdBytes(ctx: &mut dyn NativeCtx, fd: i64, data: VmValue) -> Result<i64, String> {
            let bytes = ctx.buffer_to_bytes(data).ok_or_else(|| coded("E_FS_BAD_ARG", "expected Bytes"))?;
            descriptors::write_all(&descriptors::handle(fd)?, &bytes)?;
            Ok(bytes.len() as i64)
        }

        fn readFileBytes(ctx: &mut dyn NativeCtx, path: &str) -> Result<VmValue, String> {
            allow_read(ctx, path)?;
            Ok(ctx.alloc_buffer_from_bytes(&paths::read_bytes(path)?))
        }

        fn readFileBytesAsync(ctx: &mut dyn NativeCtx, path: &str) -> Result<VmValue, String> {
            allow_read(ctx, path)?;
            let path = path.to_owned();
            Ok(blocking::task(ctx, move || paths::read_bytes(&path).map(SendValue::Bytes)))
        }
        fn writeFileBytes(ctx: &mut dyn NativeCtx, path: &str, data: VmValue) -> Result<(), String> {
            allow_write(ctx, path)?;
            paths::write(path, &bytes_arg(ctx, data)?)
        }

        fn writeFileBytesAsync(ctx: &mut dyn NativeCtx, path: &str, data: VmValue) -> Result<VmValue, String> {
            allow_write(ctx, path)?;
            let (path, bytes) = (path.to_owned(), bytes_arg(ctx, data)?);
            Ok(blocking::task(ctx, move || paths::write(&path, &bytes).map(|()| SendValue::Null)))
        }
        fn seek(ctx: &mut dyn NativeCtx, fd: i64, offset: i64, origin: VmValue) -> Result<i64, String> {
            let origin = SeekOrigin::from_value(ctx, origin)
                .ok_or_else(|| coded("E_FS_BAD_ARG", "expected a SeekOrigin"))?;
            let file_arc = descriptors::handle(fd)?;
            let mut file = file_arc.lock().unwrap_or_else(|e| e.into_inner());

            use std::io::{Seek, SeekFrom};
            let seek_from = match origin {
                SeekOrigin::Start => SeekFrom::Start(offset as u64),
                SeekOrigin::Current => SeekFrom::Current(offset),
                SeekOrigin::End => SeekFrom::End(offset),
            };
            let pos = file.seek(seek_from).map_err(fs_io_err)?;
            Ok(pos as i64)
        }

        fn close(_ctx: &mut dyn NativeCtx, fd: i64) -> Result<(), String> {
            descriptors::unregister(fd)
        }

        fn exists(ctx: &mut dyn NativeCtx, path: &str) -> Result<bool, String> {
            allow_read(ctx, path)?;
            std::path::Path::new(path).try_exists().map_err(fs_io_err)
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
                .map(|d| d.as_millis() as i64);
            Ok(RawStats {
                size: meta.len() as i64,
                is_dir: meta.is_dir(),
                is_file: meta.is_file(),
                mtime: mtime.map_or_else(VmValue::null, VmValue::from_int),
            }
            .alloc(ctx))
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
            allow_read(ctx, path)?;
            Ok(strings(ctx, paths::read_dir(path)?))
        }

        fn readDirAsync(ctx: &mut dyn NativeCtx, path: &str) -> Result<VmValue, String> {
            allow_read(ctx, path)?;
            let path = path.to_owned();
            Ok(blocking::task(ctx, move || paths::read_dir(&path).map(text_array)))
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
            allow_write(ctx, path)?;
            paths::remove_all(path)
        }

        fn removeAllAsync(ctx: &mut dyn NativeCtx, path: &str) -> Result<VmValue, String> {
            allow_write(ctx, path)?;
            let path = path.to_owned();
            Ok(blocking::task(ctx, move || paths::remove_all(&path).map(|()| SendValue::Null)))
        }
        fn rename(ctx: &mut dyn NativeCtx, from: &str, to: &str) -> Result<(), String> {
            if !ctx.check_fs_read(from) || !ctx.check_fs_write(to) {
                return Err(denied("fs.write", to));
            }
            fs::rename(from, to).map_err(fs_io_err)
        }

        fn copyFile(ctx: &mut dyn NativeCtx, from: &str, to: &str) -> Result<(), String> {
            allow_copy(ctx, from, to)?;
            paths::copy(from, to)
        }

        fn copyFileAsync(ctx: &mut dyn NativeCtx, from: &str, to: &str) -> Result<VmValue, String> {
            allow_copy(ctx, from, to)?;
            let (from, to) = (from.to_owned(), to.to_owned());
            Ok(blocking::task(ctx, move || paths::copy(&from, &to).map(|()| SendValue::Null)))
        }
        fn readFileText(ctx: &mut dyn NativeCtx, path: &str) -> Result<String, String> {
            allow_read(ctx, path)?;
            paths::read_text(path)
        }

        fn readFileTextAsync(ctx: &mut dyn NativeCtx, path: &str) -> Result<VmValue, String> {
            allow_read(ctx, path)?;
            let path = path.to_owned();
            Ok(blocking::task(ctx, move || paths::read_text(&path).map(SendValue::Str)))
        }
        fn writeFileText(ctx: &mut dyn NativeCtx, path: &str, data: &str) -> Result<(), String> {
            allow_write(ctx, path)?;
            paths::write(path, data.as_bytes())
        }

        fn writeFileTextAsync(ctx: &mut dyn NativeCtx, path: &str, data: &str) -> Result<VmValue, String> {
            allow_write(ctx, path)?;
            let (path, data) = (path.to_owned(), data.to_owned());
            Ok(blocking::task(ctx, move || paths::write(&path, data.as_bytes()).map(|()| SendValue::Null)))
        }
        fn appendFileText(ctx: &mut dyn NativeCtx, path: &str, data: &str) -> Result<(), String> {
            allow_write(ctx, path)?;
            paths::append(path, data.as_bytes())
        }

        fn appendFileTextAsync(ctx: &mut dyn NativeCtx, path: &str, data: &str) -> Result<VmValue, String> {
            allow_write(ctx, path)?;
            let (path, data) = (path.to_owned(), data.to_owned());
            Ok(blocking::task(ctx, move || paths::append(&path, data.as_bytes()).map(|()| SendValue::Null)))
        }
        fn readLines(ctx: &mut dyn NativeCtx, path: &str) -> Result<Vec<VmValue>, String> {
            allow_read(ctx, path)?;
            Ok(strings(ctx, paths::read_lines(path)?))
        }

        fn readLinesAsync(ctx: &mut dyn NativeCtx, path: &str) -> Result<VmValue, String> {
            allow_read(ctx, path)?;
            let path = path.to_owned();
            Ok(blocking::task(ctx, move || paths::read_lines(&path).map(text_array)))
        }
        fn readFdAll(_ctx: &mut dyn NativeCtx, fd: i64) -> Result<String, String> {
            let buf = descriptors::read_all(&descriptors::handle(fd)?)?;
            String::from_utf8(buf).map_err(|e| coded("E_FS_ENCODING", e))
        }

        fn readFdAllBytes(ctx: &mut dyn NativeCtx, fd: i64) -> Result<VmValue, String> {
            let buf = descriptors::read_all(&descriptors::handle(fd)?)?;
            Ok(ctx.alloc_buffer_from_bytes(&buf))
        }

        fn readFdBytesAsync(ctx: &mut dyn NativeCtx, fd: i64, len: i64) -> Result<VmValue, String> {
            let file = descriptors::handle(fd)?;
            Ok(blocking::task(ctx, move || descriptors::read_some(&file, len).map(SendValue::Bytes)))
        }

        fn readFdAllBytesAsync(ctx: &mut dyn NativeCtx, fd: i64) -> Result<VmValue, String> {
            let file = descriptors::handle(fd)?;
            Ok(blocking::task(ctx, move || descriptors::read_all(&file).map(SendValue::Bytes)))
        }

        fn writeFdBytesAsync(ctx: &mut dyn NativeCtx, fd: i64, data: VmValue) -> Result<VmValue, String> {
            let file = descriptors::handle(fd)?;
            let bytes = ctx.buffer_to_bytes(data).ok_or_else(|| coded("E_FS_BAD_ARG", "expected Bytes"))?;
            Ok(blocking::task(ctx, move || {
                descriptors::write_all(&file, &bytes).map(|()| SendValue::Int(bytes.len() as i64))
            }))
        }

        fn tempDir(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(std::env::temp_dir().to_string_lossy().into_owned())
        }
    }
}
