use parking_lot::Mutex;
use std::net::TcpStream;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};
use varn_op_macros::varn_contract;
use varn_types::NativeCtx;

type WsStream = WebSocket<MaybeTlsStream<TcpStream>>;

static WS_POOL: Mutex<Vec<Option<WsStream>>> = Mutex::new(Vec::new());

pub struct WsRuntime;

varn_contract! {
    module: "runtime:ws",
    contract: "src/modules/host/ws/ws_runtime.vn",
    impl WsRuntime {
        fn wsConnect(ctx: &mut dyn NativeCtx, url: &str) -> Result<i64, String> {
            if !ctx.check_net_connect(url) {
                return Err(format!("SecurityError: Permission denied (net.client) to ws url '{url}'"));
            }

            let (socket, _) = connect(url).map_err(|e| format!("WebSocket connection failed: {e}"))?;
            let mut pool = WS_POOL.lock();
            let id = pool.len() as i64;
            pool.push(Some(socket));
            Ok(id)
        }

        fn wsSend(_ctx: &mut dyn NativeCtx, ws_id: i64, message: &str) -> Result<bool, String> {
            if ws_id < 0 {
                return Ok(false);
            }
            let mut pool = WS_POOL.lock();
            if let Some(Some(socket)) = pool.get_mut(ws_id as usize) {
                socket.send(Message::Text(message.to_string()))
                    .map_err(|e| format!("WebSocket send error: {e}"))?;
                return Ok(true);
            }
            Ok(false)
        }

        fn wsRead(_ctx: &mut dyn NativeCtx, ws_id: i64) -> Result<Option<String>, String> {
            if ws_id < 0 {
                return Ok(None);
            }
            let mut pool = WS_POOL.lock();
            if let Some(Some(socket)) = pool.get_mut(ws_id as usize) {
                match socket.read() {
                    Ok(Message::Text(text)) => {
                        return Ok(Some(text));
                    }
                    Ok(Message::Binary(bin)) => {
                        let text = String::from_utf8_lossy(&bin).into_owned();
                        return Ok(Some(text));
                    }
                    Ok(Message::Close(_)) => {
                        return Ok(None);
                    }
                    Ok(_) => {
                        return Ok(Some(String::new()));
                    }
                    Err(e) => {
                        return Err(format!("WebSocket read error: {e}"));
                    }
                }
            }
            Ok(None)
        }

        fn wsClose(_ctx: &mut dyn NativeCtx, ws_id: i64) -> Result<bool, String> {
            if ws_id < 0 {
                return Ok(false);
            }
            let mut pool = WS_POOL.lock();
            if let Some(slot) = pool.get_mut(ws_id as usize) {
                if let Some(mut socket) = slot.take() {
                    let _ = socket.close(None);
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}
