use crate::error::{FrameInfo, RuntimeError};
use crate::frame::{CallFrame, TryHandler};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use varn_core::{IntrinsicType, TypeTag};

pub(crate) fn push_try(
    handlers: &mut Vec<TryHandler>,
    catch_ip: usize,
    frame_depth: usize,
    err_reg: u8,
) {
    handlers.push(TryHandler {
        catch_ip,
        frame_depth,
        err_reg,
    });
}

pub(crate) fn pop_try(handlers: &mut Vec<TryHandler>) {
    handlers.pop();
}

pub(crate) fn collect_frames(frames: &[CallFrame]) -> Vec<FrameInfo> {
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
            let raw_file = proto.chunk.source_file.as_ref();
            let file = if let Some(stripped) = raw_file.strip_prefix(r"\\?\") {
                stripped.to_owned()
            } else {
                raw_file.to_owned()
            };
            let raw_line = proto.chunk.lines.get_line(f.ip.saturating_sub(1));
            let line = if raw_line > 0 { raw_line } else { 1 };
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
                    if !name.is_empty()
                        && name != IntrinsicType::Error.as_str()
                        && name != TypeTag::Null.name()
                    {
                        return format!("{}: {}", name, msg);
                    }
                }
                return msg;
            }

            let class_name = obj.class_name();
            if class_name != TypeTag::Object.name() {
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
