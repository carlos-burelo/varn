use std::fs;
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct FsRuntime;

varn_contract! {
    module: "runtime:fs",
    contract: "src/modules/host/fs/fs_runtime.vn",
    impl FsRuntime {
        fn fsRead(_ctx: &mut dyn NativeCtx, path: &str) -> Result<String, String> {
            fs::read_to_string(path).map_err(|e| e.to_string())
        }
        fn fsWrite(_ctx: &mut dyn NativeCtx, path: &str, content: &str) -> Result<(), String> {
            fs::write(path, content).map_err(|e| e.to_string())
        }
        fn fsExists(_ctx: &mut dyn NativeCtx, path: &str) -> Result<bool, String> {
            Ok(std::path::Path::new(path).exists())
        }
        fn fsStat(ctx: &mut dyn NativeCtx, path: &str) -> Result<VmValue, String> {
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
        fn fsMkdir(_ctx: &mut dyn NativeCtx, path: &str) -> Result<(), String> {
            fs::create_dir(path).map_err(|e| e.to_string())
        }
        fn fsMkdirAll(_ctx: &mut dyn NativeCtx, path: &str) -> Result<(), String> {
            fs::create_dir_all(path).map_err(|e| e.to_string())
        }
        fn fsReadDir(ctx: &mut dyn NativeCtx, path: &str) -> Result<Vec<VmValue>, String> {
            let entries = fs::read_dir(path).map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    out.push(ctx.alloc_str(name));
                }
            }
            Ok(out)
        }
        fn fsRemove(_ctx: &mut dyn NativeCtx, path: &str) -> Result<(), String> {
            let p = std::path::Path::new(path);
            if p.is_dir() {
                fs::remove_dir(p).map_err(|e| e.to_string())
            } else {
                fs::remove_file(p).map_err(|e| e.to_string())
            }
        }
        fn fsRemoveAll(_ctx: &mut dyn NativeCtx, path: &str) -> Result<(), String> {
            let p = std::path::Path::new(path);
            if p.is_dir() {
                fs::remove_dir_all(p).map_err(|e| e.to_string())
            } else {
                fs::remove_file(p).map_err(|e| e.to_string())
            }
        }
        fn fsRename(_ctx: &mut dyn NativeCtx, from: &str, to: &str) -> Result<(), String> {
            fs::rename(from, to).map_err(|e| e.to_string())
        }
        fn fsTempDir(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(std::env::temp_dir().to_string_lossy().into_owned())
        }
    }
}
