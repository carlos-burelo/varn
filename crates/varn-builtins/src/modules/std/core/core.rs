use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("std:core")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn]
    pub fn describe(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let id = match args.first() {
            Some(v) if v.is_int() => v.as_int() as u64,
            Some(v) => {
                return Err(format!(
                    "core.describe: expected int op_id, got {}",
                    ctx.str_repr(*v)
                ))
            }
            None => return Err("core.describe: missing op_id argument".to_string()),
        };

        match crate::dispatch::describe_op(id) {
            None => Ok(VmValue::null()),
            Some(_meta) => Ok(VmValue::null()),
        }
    }

    #[varn_fn("opId")]
    pub fn op_id(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let name = match args.first() {
            Some(&v) => ctx.str_owned(v).ok_or("core.opId: expected str")?,
            None => return Err("core.opId: missing name argument".to_string()),
        };
        let id = if let Some(sep) = name.find("::") {
            let module_id = &name[..sep];
            let symbol = &name[sep + 2..];
            crate::dispatch::entry::compound_op_id(module_id, symbol)
        } else {
            crate::dispatch::entry::op_id(&name)
        };
        Ok(VmValue::from_int(id as i64))
    }
}
