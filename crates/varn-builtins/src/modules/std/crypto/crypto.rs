use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256, Sha512};
use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("runtime:crypto", cap = "crypto.use")]
pub(crate) mod dispatch {
    use super::*;

    fn str_arg(
        ctx: &mut dyn NativeCtx,
        args: &[VmValue],
        index: usize,
        label: &str,
    ) -> Result<String, String> {
        args.get(index)
            .map(|&v| ctx.str_repr(v))
            .ok_or_else(|| format!("crypto.{label}: expected argument {index}"))
    }

    fn bytes_to_vm_array(ctx: &mut dyn NativeCtx, bytes: &[u8]) -> VmValue {
        let values = bytes
            .iter()
            .copied()
            .map(|byte| VmValue::from_int(byte as i64))
            .collect();
        ctx.alloc_array(values)
    }

    #[varn_fn("cryptoSha256")]
    pub fn crypto_sha256(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let input = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let digest = Sha256::digest(input.as_bytes());
        Ok(ctx.alloc_str_owned(hex::encode(digest)))
    }

    #[varn_fn("cryptoSha512")]
    pub fn crypto_sha512(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let input = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let digest = Sha512::digest(input.as_bytes());
        Ok(ctx.alloc_str_owned(hex::encode(digest)))
    }

    #[varn_fn("cryptoUuid")]
    pub fn crypto_uuid(ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        use uuid::Uuid;
        Ok(ctx.alloc_str_owned(Uuid::new_v4().to_string()))
    }

    #[varn_fn("cryptoRandomBytes")]
    pub fn crypto_random_bytes(
        ctx: &mut dyn NativeCtx,
        args: &[VmValue],
    ) -> Result<VmValue, String> {
        let size = args.first().map(|&v| v.as_int()).unwrap_or_default();
        if size < 0 {
            return Err("crypto.randomBytes: size must be non-negative".to_string());
        }

        let mut bytes = vec![0u8; size as usize];
        rand::thread_rng().fill_bytes(&mut bytes);
        Ok(bytes_to_vm_array(ctx, &bytes))
    }

    #[varn_fn("cryptoRandomHex")]
    pub fn crypto_random_hex(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let size = args.first().map(|&v| v.as_int()).unwrap_or_default();
        if size < 0 {
            return Err("crypto.randomHex: size must be non-negative".to_string());
        }

        let mut bytes = vec![0u8; size as usize];
        rand::thread_rng().fill_bytes(&mut bytes);
        Ok(ctx.alloc_str_owned(hex::encode(bytes)))
    }

    #[varn_fn("cryptoBase64Enc")]
    pub fn crypto_base64_enc(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let input = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let encoded = base64::engine::general_purpose::STANDARD.encode(input.as_bytes());
        Ok(ctx.alloc_str_owned(encoded))
    }

    #[varn_fn("cryptoBase64Dec")]
    pub fn crypto_base64_dec(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let input = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(input.as_bytes())
            .map_err(|e| format!("crypto.base64_dec: {e}"))?;
        let output = String::from_utf8(decoded).map_err(|e| format!("crypto.base64_dec: {e}"))?;
        Ok(ctx.alloc_str_owned(output))
    }

    #[varn_fn("cryptoHmac")]
    pub fn crypto_hmac(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let algo = str_arg(ctx, args, 0, "hmac")?.to_lowercase();
        let key = str_arg(ctx, args, 1, "hmac")?;
        let data = str_arg(ctx, args, 2, "hmac")?;

        let digest = match algo.as_str() {
            "sha256" => {
                let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
                    .map_err(|e| format!("crypto.hmac: {e}"))?;
                mac.update(data.as_bytes());
                mac.finalize().into_bytes().to_vec()
            }
            "sha512" => {
                let mut mac = Hmac::<Sha512>::new_from_slice(key.as_bytes())
                    .map_err(|e| format!("crypto.hmac: {e}"))?;
                mac.update(data.as_bytes());
                mac.finalize().into_bytes().to_vec()
            }
            other => return Err(format!("crypto.hmac: unsupported algorithm '{other}'")),
        };

        Ok(ctx.alloc_str_owned(hex::encode(digest)))
    }
}
