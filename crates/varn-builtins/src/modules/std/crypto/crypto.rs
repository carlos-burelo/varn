use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256, Sha512};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct CryptoRuntime;

varn_contract! {
    module: "runtime:crypto",
    contract: "src/modules/std/crypto/runtime/crypto_runtime.vn",
    impl CryptoRuntime {
        fn cryptoSha256(_ctx: &mut dyn NativeCtx, data: &str) -> Result<String, String> {
            Ok(hex::encode(Sha256::digest(data.as_bytes())))
        }
        fn cryptoSha512(_ctx: &mut dyn NativeCtx, data: &str) -> Result<String, String> {
            Ok(hex::encode(Sha512::digest(data.as_bytes())))
        }
        fn cryptoUuid(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(uuid::Uuid::new_v4().to_string())
        }
        fn cryptoRandomBytes(_ctx: &mut dyn NativeCtx, size: i64) -> Result<Vec<VmValue>, String> {
            if size < 0 {
                return Err("crypto.randomBytes: size must be non-negative".to_string());
            }
            let mut bytes = vec![0u8; size as usize];
            rand::thread_rng().fill_bytes(&mut bytes);
            Ok(bytes.iter().map(|b| VmValue::from_int(*b as i64)).collect())
        }
        fn cryptoRandomHex(_ctx: &mut dyn NativeCtx, size: i64) -> Result<String, String> {
            if size < 0 {
                return Err("crypto.randomHex: size must be non-negative".to_string());
            }
            let mut bytes = vec![0u8; size as usize];
            rand::thread_rng().fill_bytes(&mut bytes);
            Ok(hex::encode(bytes))
        }
        fn cryptoBase64Enc(_ctx: &mut dyn NativeCtx, data: &str) -> Result<String, String> {
            Ok(base64::engine::general_purpose::STANDARD.encode(data.as_bytes()))
        }
        fn cryptoBase64Dec(_ctx: &mut dyn NativeCtx, data: &str) -> Result<String, String> {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(data.as_bytes())
                .map_err(|e| format!("crypto.base64_dec: {e}"))?;
            String::from_utf8(decoded).map_err(|e| format!("crypto.base64_dec: {e}"))
        }
        fn cryptoHmac(_ctx: &mut dyn NativeCtx, algo: &str, key: &str, data: &str) -> Result<String, String> {
            let digest = match algo.to_lowercase().as_str() {
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
            Ok(hex::encode(digest))
        }
    }
}
