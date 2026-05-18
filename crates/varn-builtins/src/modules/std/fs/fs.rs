use std::fs;
use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("std:fs")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn(cap = "fs.read")]
    pub fn read(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let path = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.read: expected path")?;
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        Ok(ctx.alloc_str_owned(content))
    }

    #[varn_fn(cap = "fs.write")]
    pub fn write(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let path = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.write: expected path")?;
        let content = args
            .get(1)
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.write: expected content")?;
        fs::write(path, content).map_err(|e| e.to_string())?;
        Ok(VmValue::null())
    }

    #[varn_fn(cap = "fs.read")]
    pub fn exists(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let path = args
            .first()
            .map(|&v| ctx.str_repr(v))
            .ok_or("fs.exists: expected path")?;
        Ok(VmValue::from_bool(std::path::Path::new(&path).exists()))
    }
}
