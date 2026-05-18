use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("std:crypto", cap = "crypto.use")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_group("crypto")]
    pub mod crypto_ns {
        use super::*;
        use sha2::{Digest, Sha256, Sha512};

        #[varn_fn]
        pub fn sha256(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
            let input = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
            let mut h = Sha256::new();
            h.update(input.as_bytes());
            Ok(ctx.alloc_str_owned(format!("{:x}", h.finalize())))
        }

        #[varn_fn]
        pub fn sha512(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
            let input = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
            let mut h = Sha512::new();
            h.update(input.as_bytes());
            Ok(ctx.alloc_str_owned(format!("{:x}", h.finalize())))
        }

        #[varn_fn]
        pub fn uuid(ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
            use uuid::Uuid;
            Ok(ctx.alloc_str_owned(Uuid::new_v4().to_string()))
        }
    }
}
