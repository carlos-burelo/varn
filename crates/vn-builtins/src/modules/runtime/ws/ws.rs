use base64::Engine;
use rand::RngCore;
use sha1::Digest;
use vn_core::StdErrorClass;
use vn_macros::varn_contract;
use vn_types::NativeError;
use vn_types::{NativeCtx, VmValue};

pub struct WsRuntime;

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

fn accept_key(client_key: &str) -> String {
    let mut hasher = sha1::Sha1::new();
    hasher.update(client_key.as_bytes());
    hasher.update(WS_GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

fn header_count(head: &str, name: &str) -> usize {
    let prefix = format!("{name}:");
    head.split("\r\n")
        .filter(|line| line.len() >= prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(&prefix))
        .count()
}

fn header_has_token(head: &str, name: &str, token: &str) -> bool {
    header_value(head, name).is_some_and(|value| {
        value
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case(token))
    })
}

fn protocol(message: impl Into<String>) -> NativeError {
    NativeError::std(StdErrorClass::Protocol, message)
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

vn_macros::varn_enum! {
    module: MODULE_ID,
    contract: "src/modules/runtime/ws/ws_runtime.vn",
    pub enum WsOpcode;
}

vn_macros::varn_record! {
    contract: "src/modules/runtime/ws/ws_runtime.vn",
    interface: "WsFrame",
    struct WsFrame;
}

varn_contract! {
    module: "runtime:ws",
    contract: "src/modules/runtime/ws/ws_runtime.vn",
    impl WsRuntime {
        fn wsKey(_ctx: &mut dyn NativeCtx) -> Result<String, NativeError> {
            let mut key = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut key);
            Ok(base64::engine::general_purpose::STANDARD.encode(key))
        }

        fn wsAcceptResponse(_ctx: &mut dyn NativeCtx, request: &str) -> Result<String, NativeError> {
            if !request.starts_with("GET ") {
                return Err(protocol("a WebSocket handshake is a GET request"));
            }
            if !header_has_token(request, "upgrade", "websocket") || !header_has_token(request, "connection", "upgrade") {
                return Err(protocol("not a WebSocket upgrade request"));
            }
            if header_value(request, "sec-websocket-version").as_deref() != Some("13") {
                return Err(protocol("unsupported Sec-WebSocket-Version"));
            }
            if header_count(request, "sec-websocket-key") != 1 {
                return Err(protocol("missing or repeated Sec-WebSocket-Key"));
            }
            let client_key = header_value(request, "sec-websocket-key")
                .ok_or_else(|| protocol("missing Sec-WebSocket-Key"))?;
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(client_key.as_bytes())
                .map_err(|_| protocol("Sec-WebSocket-Key is not base64"))?;
            if decoded.len() != 16 {
                return Err(protocol("Sec-WebSocket-Key must encode 16 bytes"));
            }
            let accept = accept_key(&client_key);
            Ok(format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
            ))
        }

        fn wsValidateResponse(_ctx: &mut dyn NativeCtx, head: &str, key: &str) -> Result<(), NativeError> {
            let status_line = head.split("\r\n").next().unwrap_or("");
            let mut parts = status_line.split(' ');
            let version_ok = parts.next().is_some_and(|v| v.starts_with("HTTP/1."));
            if !version_ok || parts.next() != Some("101") {
                return Err(protocol(format!("expected 101, got '{status_line}'")));
            }
            if !header_has_token(head, "upgrade", "websocket") || !header_has_token(head, "connection", "upgrade") {
                return Err(protocol("the response is not a WebSocket upgrade"));
            }
            if header_value(head, "sec-websocket-extensions").is_some() || header_value(head, "sec-websocket-protocol").is_some() {
                return Err(protocol("the server chose an extension or subprotocol that was not requested"));
            }
            if header_count(head, "sec-websocket-accept") != 1 {
                return Err(protocol("missing or repeated Sec-WebSocket-Accept"));
            }
            let expected = accept_key(key);
            match header_value(head, "sec-websocket-accept") {
                Some(got) if got == expected => Ok(()),
                Some(_) | None => Err(protocol("bad Sec-WebSocket-Accept")),
            }
        }

        fn wsDecodeText(ctx: &mut dyn NativeCtx, payload: VmValue) -> Result<String, NativeError> {
            let data = ctx
                .buffer_to_bytes(payload)
                .ok_or_else(|| NativeError::type_error("expected Bytes"))?;
            String::from_utf8(data).map_err(|_| protocol("a text message is not valid UTF-8"))
        }

        fn wsFrameBytes(ctx: &mut dyn NativeCtx, payload: VmValue, opcode: VmValue, mask: bool) -> Result<VmValue, NativeError> {
            let opcode = match WsOpcode::from_value(ctx, opcode) {
                Some(WsOpcode::Continuation) | None => {
                    return Err(NativeError::type_error("expected a sendable WsOpcode"));
                }
                Some(op @ (WsOpcode::Text | WsOpcode::Binary | WsOpcode::Close | WsOpcode::Ping | WsOpcode::Pong)) => op.tag(),
            };
            let data = ctx
                .buffer_to_bytes(payload)
                .ok_or_else(|| NativeError::type_error("expected Bytes"))?;
            if opcode >= 8 && data.len() > 125 {
                return Err(NativeError::range_error("a control frame carries at most 125 bytes"));
            }
            let mask_bit: u8 = if mask { 0x80 } else { 0 };
            let mut key = [0u8; 4];
            rand::thread_rng().fill_bytes(&mut key);
            let mut out = Vec::with_capacity(14 + data.len());
            out.push(0x80 | (opcode as u8));
            if data.len() < 126 {
                out.push(mask_bit | (data.len() as u8));
            } else if data.len() < 65536 {
                out.push(mask_bit | 126);
                out.extend_from_slice(&(data.len() as u16).to_be_bytes());
            } else {
                out.push(mask_bit | 127);
                out.extend_from_slice(&(data.len() as u64).to_be_bytes());
            }
            if mask {
                out.extend_from_slice(&key);
                for (i, b) in data.iter().enumerate() {
                    out.push(b ^ key[i % 4]);
                }
            } else {
                out.extend_from_slice(&data);
            }
            Ok(ctx.alloc_buffer_from_bytes(&out))
        }

        fn wsParseFrame(ctx: &mut dyn NativeCtx, data: VmValue, expectMasked: bool, maxPayload: i64) -> Result<Option<VmValue>, NativeError> {
            let bytes = ctx
                .buffer_to_bytes(data)
                .ok_or_else(|| NativeError::type_error("expected Bytes"))?;
            if bytes.len() < 2 {
                return Ok(None);
            }
            let fin = bytes[0] & 0x80 != 0;
            if bytes[0] & 0x70 != 0 {
                return Err(protocol("a frame sets reserved bits without a negotiated extension"));
            }
            let opcode = bytes[0] & 0x0F;
            let masked = bytes[1] & 0x80 != 0;
            if masked != expectMasked {
                return Err(protocol(if expectMasked {
                    "a client frame must be masked"
                } else {
                    "a server frame must not be masked"
                }));
            }
            let limit = u64::try_from(maxPayload).unwrap_or(0);
            let mut pos = 2usize;
            let mut len = (bytes[1] & 0x7F) as usize;
            if len == 126 {
                if bytes.len() < 4 {
                    return Ok(None);
                }
                len = u16::from_be_bytes([bytes[2], bytes[3]]) as usize;
                if len < 126 {
                    return Err(protocol("a frame length is not minimally encoded"));
                }
                pos = 4;
            } else if len == 127 {
                if bytes.len() < 10 {
                    return Ok(None);
                }
                let wide = u64::from_be_bytes([
                    bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8],
                    bytes[9],
                ]);
                if wide >> 63 != 0 || wide < 65536 {
                    return Err(protocol("a 64-bit frame length is invalid or not minimally encoded"));
                }
                if wide > limit {
                    return Err(protocol(format!("a frame of {wide} bytes exceeds the limit of {limit}")));
                }
                len = wide as usize;
                pos = 10;
            }
            if len as u64 > limit {
                return Err(protocol(format!("a frame of {len} bytes exceeds the limit of {limit}")));
            }
            let mut mask = [0u8; 4];
            if masked {
                if bytes.len() < pos + 4 {
                    return Ok(None);
                }
                mask = [bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]];
                pos += 4;
            }
            let is_control = opcode >= 8;
            if is_control && (!fin || len > 125) {
                return Err(NativeError::std(StdErrorClass::Protocol, "bad control frame"));
            }
            if bytes.len() < pos + len {
                return Ok(None);
            }
            let mut payload = bytes[pos..pos + len].to_vec();
            if masked {
                for (i, b) in payload.iter_mut().enumerate() {
                    *b ^= mask[i % 4];
                }
            }
            let opcode = WsOpcode::of_tag(opcode as i64)
                .ok_or_else(|| format!("bad opcode {opcode}")).map_err(|e| NativeError::std(StdErrorClass::Protocol, e))?
                .to_value(ctx).map_err(|e| NativeError::std(StdErrorClass::Protocol, e))?;
            let payload = ctx.alloc_buffer_from_bytes(&payload);
            Ok(Some(
                WsFrame {
                    opcode,
                    fin,
                    payload,
                    consumed: (pos + len) as i64,
                }
                .alloc(ctx),
            ))
        }
    }
}
