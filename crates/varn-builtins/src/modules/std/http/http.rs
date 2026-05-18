use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("std:http")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_group("Http")]
    pub mod http_ns {
        use super::*;

        #[varn_fn("fetch", cap = "network.client")]
        pub fn fetch(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
            let url = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
            Ok(ctx.alloc_str_owned(format!("fetch result for {}", url)))
        }
    }
}
