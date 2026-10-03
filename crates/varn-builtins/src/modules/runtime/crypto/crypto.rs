use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256, Sha512};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct CryptoRuntime;

varn_contract! {
    module: "runtime:crypto",
    contract: "src/modules/runtime/crypto/crypto_runtime.vn",
    impl CryptoRuntime {
        fn sha256(_ctx: &mut dyn NativeCtx, data: &str) -> Result<String, String> {
            Ok(hex::encode(Sha256::digest(data.as_bytes())))
        }
        fn sha512(_ctx: &mut dyn NativeCtx, data: &str) -> Result<String, String> {
            Ok(hex::encode(Sha512::digest(data.as_bytes())))
        }
        fn uuid(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(uuid::Uuid::new_v4().to_string())
        }
        fn uuidV4(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(uuid::Uuid::new_v4().to_string())
        }
        fn uuidV7(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(uuid::Uuid::now_v7().to_string())
        }
        fn uuidValidate(_ctx: &mut dyn NativeCtx, s: &str) -> Result<bool, String> {
            Ok(uuid::Uuid::parse_str(s).is_ok())
        }
        fn randomBytes(_ctx: &mut dyn NativeCtx, size: i64) -> Result<Vec<VmValue>, String> {
            if size < 0 {
                return Err("crypto.randomBytes: size must be non-negative".to_string());
            }
            let mut bytes = vec![0u8; size as usize];
            rand::thread_rng().fill_bytes(&mut bytes);
            Ok(bytes.iter().map(|b| VmValue::from_int(*b as i64)).collect())
        }
        fn randomHex(_ctx: &mut dyn NativeCtx, size: i64) -> Result<String, String> {
            if size < 0 {
                return Err("crypto.randomHex: size must be non-negative".to_string());
            }
            let mut bytes = vec![0u8; size as usize];
            rand::thread_rng().fill_bytes(&mut bytes);
            Ok(hex::encode(bytes))
        }
        fn base64Enc(_ctx: &mut dyn NativeCtx, data: &str) -> Result<String, String> {
            Ok(base64::engine::general_purpose::STANDARD.encode(data.as_bytes()))
        }
        fn base64Dec(_ctx: &mut dyn NativeCtx, data: &str) -> Result<String, String> {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(data.as_bytes())
                .map_err(|e| format!("crypto.base64_dec: {e}"))?;
            String::from_utf8(decoded).map_err(|e| format!("crypto.base64_dec: {e}"))
        }
        fn base64EncBytes(ctx: &mut dyn NativeCtx, data: VmValue) -> Result<String, String> {
            let bytes = ctx
                .buffer_to_bytes(data)
                .ok_or_else(|| "crypto.base64EncBytes: expected Bytes".to_string())?;
            Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
        }
        fn base64DecBytes(ctx: &mut dyn NativeCtx, data: &str) -> Result<VmValue, String> {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(data.as_bytes())
                .map_err(|e| format!("crypto.base64_dec: {e}"))?;
            Ok(ctx.alloc_buffer_from_bytes(&decoded))
        }
        fn hmac(_ctx: &mut dyn NativeCtx, algo: &str, key: &str, data: &str) -> Result<String, String> {
            let digest = match algo.to_lowercase().as_str() {
                "sha1" => {
                    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(key.as_bytes())
                        .map_err(|e| format!("crypto.hmac: {e}"))?;
                    mac.update(data.as_bytes());
                    mac.finalize().into_bytes().to_vec()
                }
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

        fn hmacBytes(ctx: &mut dyn NativeCtx, algo: &str, key: VmValue, data: VmValue) -> Result<VmValue, String> {
            let key_bytes = if let Some(b) = ctx.buffer_to_bytes(key) {
                b
            } else if let Some(s) = ctx.str_owned(key) {
                s.into_bytes()
            } else {
                ctx.str_repr(key).into_bytes()
            };
            let data_bytes = if let Some(b) = ctx.buffer_to_bytes(data) {
                b
            } else if let Some(s) = ctx.str_owned(data) {
                s.into_bytes()
            } else {
                ctx.str_repr(data).into_bytes()
            };
            let digest = match algo.to_lowercase().as_str() {
                "sha1" => {
                    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(&key_bytes)
                        .map_err(|e| format!("crypto.hmac: {e}"))?;
                    mac.update(&data_bytes);
                    mac.finalize().into_bytes().to_vec()
                }
                "sha256" => {
                    let mut mac = Hmac::<Sha256>::new_from_slice(&key_bytes)
                        .map_err(|e| format!("crypto.hmac: {e}"))?;
                    mac.update(&data_bytes);
                    mac.finalize().into_bytes().to_vec()
                }
                "sha512" => {
                    let mut mac = Hmac::<Sha512>::new_from_slice(&key_bytes)
                        .map_err(|e| format!("crypto.hmac: {e}"))?;
                    mac.update(&data_bytes);
                    mac.finalize().into_bytes().to_vec()
                }
                other => return Err(format!("crypto.hmac: unsupported algorithm '{other}'")),
            };
            Ok(ctx.alloc_buffer_from_bytes(&digest))
        }

        fn randomBytesBuffer(ctx: &mut dyn NativeCtx, size: i64) -> Result<VmValue, String> {
            if size < 0 {
                return Err("crypto.randomBytes: size must be non-negative".to_string());
            }
            let mut bytes = vec![0u8; size as usize];
            rand::thread_rng().fill_bytes(&mut bytes);
            Ok(ctx.alloc_buffer_from_bytes(&bytes))
        }

        fn pbkdf2(
            ctx: &mut dyn NativeCtx,
            password: &str,
            salt: VmValue,
            iterations: i64,
            key_len: i64,
        ) -> Result<VmValue, String> {
            let salt_bytes = if let Some(b) = ctx.buffer_to_bytes(salt) {
                b
            } else if let Some(s) = ctx.str_owned(salt) {
                s.into_bytes()
            } else {
                ctx.str_repr(salt).into_bytes()
            };
            if iterations <= 0 {
                return Err("PBKDF2: iterations must be positive".to_string());
            }
            if key_len <= 0 {
                return Err("PBKDF2: keyLen must be positive".to_string());
            }

            let mut out = vec![0u8; key_len as usize];
            let hmac_len = 32usize;
            let num_blocks = (key_len as usize).div_ceil(hmac_len);
            let mut block_buf = Vec::with_capacity(salt_bytes.len() + 4);

            for i in 1..=num_blocks {
                let mut mac = Hmac::<Sha256>::new_from_slice(password.as_bytes())
                    .map_err(|e| e.to_string())?;
                block_buf.clear();
                block_buf.extend_from_slice(&salt_bytes);
                block_buf.extend_from_slice(&(i as u32).to_be_bytes());
                mac.update(&block_buf);
                let mut u = mac.finalize().into_bytes();
                let mut t = u;

                for _ in 1..iterations {
                    let mut next_mac = Hmac::<Sha256>::new_from_slice(password.as_bytes())
                        .map_err(|e| e.to_string())?;
                    next_mac.update(&u);
                    u = next_mac.finalize().into_bytes();
                    for (tb, ub) in t.iter_mut().zip(u.iter()) {
                        *tb ^= *ub;
                    }
                }

                let start = (i - 1) * hmac_len;
                let end = (i * hmac_len).min(key_len as usize);
                let take = end - start;
                out[start..end].copy_from_slice(&t[..take]);
            }
            Ok(ctx.alloc_buffer_from_bytes(&out))
        }

        fn timingSafeEqual(ctx: &mut dyn NativeCtx, a: VmValue, b: VmValue) -> Result<bool, String> {
            let bytes_a = if let Some(buf) = ctx.buffer_to_bytes(a) {
                buf
            } else {
                ctx.str_repr(a).into_bytes()
            };
            let bytes_b = if let Some(buf) = ctx.buffer_to_bytes(b) {
                buf
            } else {
                ctx.str_repr(b).into_bytes()
            };
            if bytes_a.len() != bytes_b.len() {
                return Ok(false);
            }
            let mut diff = 0u8;
            for (x, y) in bytes_a.iter().zip(bytes_b.iter()) {
                diff |= x ^ y;
            }
            Ok(diff == 0)
        }
    }
}
