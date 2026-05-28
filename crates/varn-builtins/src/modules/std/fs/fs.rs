use std::fs;
use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("runtime:fs")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn("fsRead")]
    pub fn read(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let path = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.read: expected path")?;
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        Ok(ctx.alloc_str_owned(content))
    }

    #[varn_fn("fsWrite")]
    pub fn write(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let path = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.write: expected path")?;
        let content = args
            .get(1)
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.write: expected content")?;
        fs::write(&path, &content).map_err(|e| e.to_string())?;
        Ok(VmValue::null())
    }

    #[varn_fn("fsExists")]
    pub fn exists(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let path = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.exists: expected path")?;
        let exists = std::path::Path::new(&path).exists();
        Ok(VmValue::from_bool(exists))
    }

    #[varn_fn("fsStat")]
    pub fn stat(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let path = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.stat: expected path")?;
        let meta = fs::metadata(&path).map_err(|e| e.to_string())?;
        
        let mtime = meta.modified()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))
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

    #[varn_fn("fsMkdir")]
    pub fn mkdir(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let path = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.mkdir: expected path")?;
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        Ok(VmValue::null())
    }

    #[varn_fn("fsMkdirAll")]
    pub fn mkdir_all(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let path = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.mkdirAll: expected path")?;
        fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        Ok(VmValue::null())
    }

    #[varn_fn("fsReadDir")]
    pub fn read_dir(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let path = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.readDir: expected path")?;
        let entries = fs::read_dir(&path).map_err(|e| e.to_string())?;
        let mut file_names = Vec::new();
        for entry in entries {
            if let Ok(entry) = entry {
                if let Some(name) = entry.file_name().to_str() {
                    file_names.push(ctx.alloc_str(name));
                }
            }
        }
        Ok(ctx.alloc_array(file_names))
    }

    #[varn_fn("fsRemove")]
    pub fn remove(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let path = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.remove: expected path")?;
        let path_ref = std::path::Path::new(&path);
        if path_ref.is_dir() {
            fs::remove_dir(path_ref).map_err(|e| e.to_string())?;
        } else {
            fs::remove_file(path_ref).map_err(|e| e.to_string())?;
        }
        Ok(VmValue::null())
    }

    #[varn_fn("fsRemoveAll")]
    pub fn remove_all(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let path = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.removeAll: expected path")?;
        let path_ref = std::path::Path::new(&path);
        if path_ref.is_dir() {
            fs::remove_dir_all(path_ref).map_err(|e| e.to_string())?;
        } else {
            fs::remove_file(path_ref).map_err(|e| e.to_string())?;
        }
        Ok(VmValue::null())
    }

    #[varn_fn("fsRename")]
    pub fn rename(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let from = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.rename: expected from path")?;
        let to = args
            .get(1)
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.rename: expected to path")?;
        fs::rename(&from, &to).map_err(|e| e.to_string())?;
        Ok(VmValue::null())
    }

    #[varn_fn("fsTempDir")]
    pub fn temp_dir(ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        let path = std::env::temp_dir().to_string_lossy().into_owned();
        Ok(ctx.alloc_str_owned(path))
    }
}
