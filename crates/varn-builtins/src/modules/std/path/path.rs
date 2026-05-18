use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("std:path")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn]
    pub fn normalize(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let p = std::path::Path::new(&s);
        let mut comps: Vec<&str> = vec![];
        for comp in p.components() {
            use std::path::Component;
            match comp {
                Component::ParentDir => {
                    comps.pop();
                }
                Component::CurDir => {}
                Component::Normal(s) => comps.push(s.to_str().unwrap_or("")),
                Component::RootDir => comps.push(""),
                Component::Prefix(p) => comps.push(p.as_os_str().to_str().unwrap_or("")),
            }
        }
        Ok(ctx.alloc_str_owned(comps.join(std::path::MAIN_SEPARATOR_STR)))
    }

    #[varn_fn]
    pub fn dirname(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let dir = std::path::Path::new(&s)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".to_owned());
        Ok(ctx.alloc_str_owned(dir))
    }

    #[varn_fn]
    pub fn basename(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let name = std::path::Path::new(&s)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(ctx.alloc_str_owned(name))
    }
}
