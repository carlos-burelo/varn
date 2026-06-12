use crate::error::{FrameInfo, RuntimeError};
use crate::frame::{CallFrame, TryHandler};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

pub fn push_try(handlers: &mut Vec<TryHandler>, catch_ip: usize, frame_depth: usize, err_reg: u8) {
    handlers.push(TryHandler {
        catch_ip,
        frame_depth,
        err_reg,
    });
}

pub fn pop_try(handlers: &mut Vec<TryHandler>) {
    handlers.pop();
}

pub fn dispatch_error(
    handlers: &mut Vec<TryHandler>,
    err: RuntimeError,
) -> Result<(usize, usize, VmValue), RuntimeError> {
    if let Some(handler) = handlers.pop() {
        Ok((handler.catch_ip, handler.frame_depth, VmValue::null()))
    } else {
        Err(err)
    }
}

pub fn collect_frames(frames: &[CallFrame]) -> Vec<FrameInfo> {
    frames
        .iter()
        .rev()
        .filter_map(|f| {
            let proto = &f.closure().proto;
            let fn_name = proto
                .name
                .as_deref()
                .map(|s| s.to_owned())
                .unwrap_or_else(|| "<anonymous>".to_owned());
            let file = proto.chunk.source_file.to_string();
            let line = proto.chunk.lines.get_line(f.ip.saturating_sub(1));
            Some(FrameInfo {
                fn_name,
                file,
                line,
            })
        })
        .collect()
}

fn extract_error_message(val: VmValue, heap: &Heap) -> String {
    if val.is_heap() {
        if let Some(HeapObj::Object(obj_ref)) = heap.get(val.as_heap_idx()) {
            let obj = obj_ref.borrow();

            if let Some(msg_nv) = obj.get_field_nv("message") {
                let msg = heap.str_repr(msg_nv);

                if let Some(name_nv) = obj.get_field_nv("name") {
                    let name = heap.str_repr(name_nv);
                    if !name.is_empty() && name != "Error" && name != "null" {
                        return format!("{}: {}", name, msg);
                    }
                }
                return msg;
            }

            let class_name = obj.class_name();
            if class_name != "object" {
                return format!("[{}]", class_name);
            }
            return "[object Object]".into();
        }
    }
    heap.str_repr(val)
}

pub fn build_thrown_error(val: VmValue, heap: &Heap, frames: &[CallFrame]) -> RuntimeError {
    let msg = extract_error_message(val, heap);
    let frame_infos = collect_frames(frames);
    RuntimeError {
        message: msg,
        frames: frame_infos,
        thrown: Some(val),
    }
}
