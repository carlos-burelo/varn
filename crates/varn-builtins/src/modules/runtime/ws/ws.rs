use base64::Engine;
use rand::RngCore;
use sha1::Digest;
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct WsRuntime;

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

fn coded(code: &str, msg: impl std::fmt::Display) -> String {
    format!("{code}|{msg}")
}

fn accept_key(client_key: &str) -> String {
    let mut hasher = sha1::Sha1::new();
    hasher.update(client_key.as_bytes());
    hasher.update(WS_GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

fn header_value(head: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}:");
    for line in head.split("\r\n") {
        if line.len() >= prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            return Some(line[prefix.len()..].trim().to_string());
        }
    }
    None
}

varn_contract! {
    module: "runtime:ws",
    contract: "src/modules/runtime/ws/ws_runtime.vn",
    impl WsRuntime {
        fn wsKey(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            let mut key = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut key);
            Ok(base64::engine::general_purpose::STANDARD.encode(key))
        }

        fn wsAcceptResponse(_ctx: &mut dyn NativeCtx, request: &str) -> Result<String, String> {
            let client_key = header_value(request, "sec-websocket-key")
                .ok_or_else(|| coded("E_WS_HANDSHAKE", "missing Sec-WebSocket-Key"))?;
            let accept = accept_key(&client_key);
            Ok(format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
            ))
        }

        fn wsValidateResponse(_ctx: &mut dyn NativeCtx, head: &str, key: &str) -> Result<(), String> {
            let status_line = head.split("\r\n").next().unwrap_or("");
            if !status_line.contains("101") {
                return Err(coded("E_WS_CONNECT", format!("expected 101, got '{status_line}'")));
            }
            let expected = accept_key(key);
            match header_value(head, "sec-websocket-accept") {
                Some(got) if got == expected => Ok(()),
                _ => Err(coded("E_WS_CONNECT", "bad Sec-WebSocket-Accept")),
            }
        }

        fn wsFrameBytes(ctx: &mut dyn NativeCtx, payload: VmValue, opcode: i64) -> Result<VmValue, String> {
            if ![1, 2, 8, 9, 10].contains(&opcode) {
                return Err(coded("E_WS_BAD_ARG", format!("bad opcode {opcode}")));
            }
            let data = ctx
                .buffer_to_bytes(payload)
                .ok_or_else(|| coded("E_WS_BAD_ARG", "expected Bytes"))?;
            let mut mask = [0u8; 4];
            rand::thread_rng().fill_bytes(&mut mask);
            let mut out = Vec::with_capacity(14 + data.len());
            out.push(0x80 | (opcode as u8));
            if data.len() < 126 {
                out.push(0x80 | (data.len() as u8));
            } else if data.len() < 65536 {
                out.push(0x80 | 126);
                out.extend_from_slice(&(data.len() as u16).to_be_bytes());
            } else {
                out.push(0x80 | 127);
                out.extend_from_slice(&(data.len() as u64).to_be_bytes());
            }
            out.extend_from_slice(&mask);
            for (i, b) in data.iter().enumerate() {
                out.push(b ^ mask[i % 4]);
            }
            Ok(ctx.alloc_buffer_from_bytes(&out))
        }

        fn wsParseFrame(ctx: &mut dyn NativeCtx, data: VmValue) -> Result<VmValue, String> {
            let bytes = ctx
                .buffer_to_bytes(data)
                .ok_or_else(|| coded("E_WS_BAD_ARG", "expected Bytes"))?;
            if bytes.len() < 2 {
                return Ok(VmValue::null());
            }
            let fin = bytes[0] & 0x80 != 0;
            let opcode = bytes[0] & 0x0F;
            let masked = bytes[1] & 0x80 != 0;
            let mut pos = 2usize;
            let mut len = (bytes[1] & 0x7F) as usize;
            if len == 126 {
                if bytes.len() < 4 {
                    return Ok(VmValue::null());
                }
                len = u16::from_be_bytes([bytes[2], bytes[3]]) as usize;
                pos = 4;
            } else if len == 127 {
                if bytes.len() < 10 {
                    return Ok(VmValue::null());
                }
                let wide = u64::from_be_bytes([
                    bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8],
                    bytes[9],
                ]);
                if wide > (isize::MAX as u64) {
                    return Err(coded("E_WS_PROTOCOL", "frame too large"));
                }
                len = wide as usize;
                pos = 10;
            }
            let mut mask = [0u8; 4];
            if masked {
                if bytes.len() < pos + 4 {
                    return Ok(VmValue::null());
                }
                mask = [bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]];
                pos += 4;
            }
            let is_control = opcode >= 8;
            if is_control && (!fin || len > 125) {
                return Err(coded("E_WS_PROTOCOL", "bad control frame"));
            }
            if bytes.len() < pos + len {
                return Ok(VmValue::null());
            }
            let mut payload = bytes[pos..pos + len].to_vec();
            if masked {
                for (i, b) in payload.iter_mut().enumerate() {
                    *b ^= mask[i % 4];
                }
            }
            let kind = match opcode {
                0 => "continuation",
                1 => "text",
                2 => "binary",
                8 => "close",
                9 => "ping",
                10 => "pong",
                _ => return Err(coded("E_WS_PROTOCOL", format!("bad opcode {opcode}"))),
            };
            let obj = ctx.alloc_object();
            let kind_nv = ctx.alloc_str(kind);
            ctx.set_field(obj, "kind", kind_nv);
            ctx.set_field(obj, "opcode", VmValue::from_int(opcode as i64));
            ctx.set_field(obj, "fin", VmValue::from_bool(fin));
            let payload_nv = ctx.alloc_buffer_from_bytes(&payload);
            ctx.set_field(obj, "payload", payload_nv);
            let consumed_nv = VmValue::from_int((pos + len) as i64);
            ctx.set_field(obj, "consumed", consumed_nv);
            Ok(obj)
        }
    }
}
