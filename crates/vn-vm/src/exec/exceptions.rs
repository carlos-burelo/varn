use crate::error::{FrameInfo, RuntimeError};
use crate::frame::{CallFrame, TryHandler};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use vn_core::RuntimeKind;

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
    let mut out = Vec::with_capacity(frames.len());
    for frame in frames.iter().rev() {
        frame_chain(&frame.closure().proto, frame.ip, &mut out);
    }
    out
}

pub(crate) fn frame_chain(proto: &vn_types::FunctionProto, ip: usize, out: &mut Vec<FrameInfo>) {
    let function_name = proto
        .name
        .as_deref()
        .map_or_else(|| "<anonymous>".to_owned(), str::to_owned);
    let file = proto.chunk.source_file.as_ref().to_owned();
    let at = ip.saturating_sub(1);
    let mut scope = proto.chunk.lines.get_scope(at);
    let mut line = proto.chunk.lines.get_line(at).max(1);
    loop {
        let enclosing = if scope == 0 {
            None
        } else {
            proto.chunk.inline_scopes.get(scope as usize - 1)
        };
        out.push(FrameInfo {
            fn_name: enclosing.map_or_else(|| function_name.clone(), |s| s.function.clone()),
            file: file.clone(),
            line,
        });
        let Some(enclosing) = enclosing else { break };
        line = enclosing.call_line.max(1);
        scope = enclosing.parent;
    }
}

struct ThrownParts {
    class: String,
    message: String,
    cause: Option<VmValue>,
    stack: String,
}

fn describe_thrown(val: VmValue, heap: &Heap) -> ThrownParts {
    let plain = |message: String| ThrownParts {
        class: vn_core::RuntimeErrorKind::Error.class_name().to_owned(),
        message,
        cause: None,
        stack: String::new(),
    };
    if !val.is_heap() {
        return plain(heap.str_repr(val));
    }
    if let Some(inst) = heap.instance(val.as_heap())
        && let Some(cls) = vn_types::ClassObj::find_by_id(inst.class_id)
    {
        let layout = cls.layout();
        let field = |key: vn_core::MemberKey| {
            layout
                .get_field(key.as_str())
                .and_then(|f| inst.read_field(f))
        };
        return ThrownParts {
            class: cls.name.to_string(),
            message: field(vn_core::MemberKey::Message)
                .map(|nv| heap.str_repr(nv))
                .unwrap_or_default(),
            cause: field(vn_core::MemberKey::Cause).filter(|c| !c.is_null()),
            stack: field(vn_core::MemberKey::Stack)
                .map(|nv| heap.str_repr(nv))
                .unwrap_or_default(),
        };
    }
    if let Some(HeapObj::Object(obj_ref)) = heap.get(val.as_heap()) {
        let obj = obj_ref.borrow();
        let message = obj
            .get_field(vn_core::MemberKey::Message.as_str())
            .map(|nv| heap.str_repr(nv));
        let stack = obj
            .get_field(vn_core::MemberKey::Stack.as_str())
            .map(|nv| heap.str_repr(nv))
            .unwrap_or_default();
        let named = obj
            .get_field(vn_core::MemberKey::Name.as_str())
            .map(|nv| heap.str_repr(nv))
            .filter(|name| !name.is_empty() && name != RuntimeKind::Null.name());
        return match (message, named) {
            (Some(message), Some(class)) => ThrownParts {
                class,
                message,
                cause: None,
                stack,
            },
            (Some(message), None) => plain(message),
            (None, _) => {
                let class_name = obj.class_name();
                if class_name == RuntimeKind::Object.name() {
                    plain("[object Object]".to_owned())
                } else {
                    plain(format!("[{class_name}]"))
                }
            }
        };
    }
    plain(heap.str_repr(val))
}

pub(crate) fn format_stack(frames: &[FrameInfo]) -> String {
    frames
        .iter()
        .map(|frame| {
            format!(
                "  at {} ({}:{})",
                frame.fn_name,
                vn_core::diagnostics::display_path(&frame.file),
                frame.line
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_stack(text: &str) -> Vec<FrameInfo> {
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("at ")?;
            let (name, location) = rest.split_once(" (")?;
            let location = location.strip_suffix(')')?;
            let (file, line_no) = location.rsplit_once(':')?;
            Some(FrameInfo {
                fn_name: name.to_owned(),
                file: file.to_owned(),
                line: line_no.parse().ok()?,
            })
        })
        .collect()
}

pub(crate) fn stamp_thrown(heap: &mut Heap, val: VmValue, frames: &[CallFrame]) {
    stamp_stack(heap, val, &collect_frames(frames));
}

fn stamp_stack(heap: &mut Heap, val: VmValue, frames: &[FrameInfo]) {
    if !val.is_heap() || frames.is_empty() {
        return;
    }
    let Some(class_id) = heap.instance(val.as_heap()).map(|inst| inst.class_id) else {
        return;
    };
    let Some(cls) = vn_types::ClassObj::find_by_id(class_id) else {
        return;
    };
    let layout = cls.layout();
    let Some(field) = layout.get_field(vn_core::MemberKey::Stack.as_str()) else {
        return;
    };
    let unset = heap
        .instance(val.as_heap())
        .and_then(|inst| inst.read_field(field))
        .is_none_or(|current| heap.str_repr(current).is_empty());
    if !unset {
        return;
    }
    let text = heap.alloc_str_dynamic(format_stack(frames));
    if let Some(inst) = heap.instance(val.as_heap()) {
        let _ = inst.write_field(field, text);
    }
}

const MAX_CAUSE_DEPTH: usize = 8;

pub fn build_thrown_error(val: VmValue, heap: &Heap, frames: &[CallFrame]) -> RuntimeError {
    build_thrown_chain(val, heap, collect_frames(frames), MAX_CAUSE_DEPTH)
}

fn build_thrown_chain(
    val: VmValue,
    heap: &Heap,
    frames: Vec<FrameInfo>,
    remaining: usize,
) -> RuntimeError {
    let parts = describe_thrown(val, heap);
    let frames = if frames.is_empty() {
        parse_stack(&parts.stack)
    } else {
        frames
    };
    let cause = parts
        .cause
        .filter(|_| remaining > 0)
        .map(|inner| Box::new(build_thrown_chain(inner, heap, Vec::new(), remaining - 1)));
    RuntimeError {
        class: parts.class,
        message: parts.message,
        frames,
        cause,
        thrown: Some(val),
        kind: vn_core::RuntimeErrorKind::Error,
        std: None,
    }
}

pub(crate) fn thrown_value_for(err: &RuntimeError, heap: &mut Heap) -> VmValue {
    if let Some(v) = err.thrown {
        return v;
    }
    let msg = heap.alloc_str_dynamic(&err.message);
    let std_class = err
        .std
        .as_ref()
        .filter(|info| heap.get_intrinsic_class(info.class.name()).is_some());
    let class_name = std_class
        .map(|info| info.class.name())
        .unwrap_or_else(|| err.kind.class_name());
    let Some(cls) = heap.get_intrinsic_class(class_name) else {
        return msg;
    };
    let name = heap.alloc_str_dynamic(class_name);
    let stack = heap.alloc_str_dynamic(format_stack(&err.frames));
    let (shape, n) = cls.instance_shape();
    let err_obj = heap.alloc_object_cell(shape, n, &[]);
    if let Some(HeapObj::Object(oref)) = heap.get(err_obj.as_heap()) {
        oref.set_field(
            std::sync::Arc::from(vn_core::MemberKey::Message.as_str()),
            msg,
        );
        oref.set_field(
            std::sync::Arc::from(vn_core::MemberKey::Name.as_str()),
            name,
        );
        oref.set_field(
            std::sync::Arc::from(vn_core::MemberKey::Stack.as_str()),
            stack,
        );
    }
    if let Some(info) = std_class {
        let code = heap.alloc_str_dynamic(info.class.code());
        let mut entries = vec![(vn_core::ErrorField::Code.name(), code)];
        for (field, value) in &info.fields {
            let value = match value {
                vn_types::ErrorValue::Str(text) => heap.alloc_str_dynamic(text),
                vn_types::ErrorValue::Int(n) => VmValue::from_int(*n),
            };
            entries.push((field.name(), value));
        }
        if let Some(HeapObj::Object(oref)) = heap.get(err_obj.as_heap()) {
            for (key, value) in entries {
                oref.set_field(std::sync::Arc::from(key), value);
            }
        }
    }
    err_obj
}

#[allow(dangerous_implicit_autorefs)]
pub(crate) unsafe fn dispatch_to_handler(
    ctx: *mut crate::exec::ExecCtx,
    thrown_val: VmValue,
    depth: usize,
) -> bool {
    let ctx = unsafe { &mut *ctx };
    if let Some(handler) = ctx.try_handlers.pop_if(|h| h.frame_depth > depth) {
        if crate::exec::frame_ctrl::unwind_to_handler(&mut *ctx, handler, thrown_val).is_err() {
            return false;
        }
        return true;
    }

    while ctx.frames.len() > depth {
        let cur_ip = ctx.frames.last().unwrap().ip as u32;
        let proto = &ctx.frames.last().unwrap().closure().proto;
        let hit = proto
            .exception_table
            .iter()
            .find(|r| cur_ip >= r.try_start_ip && cur_ip <= r.try_end_ip)
            .map(|r| (r.catch_ip as usize, r.err_reg as usize));
        let Some((catch_ip, err_reg)) = hit else {
            let popped = ctx.frames.pop().unwrap();
            (*ctx).drop_frame_storage(popped.base);
            if popped.awaited && ctx.frames.len() > depth {
                ctx.reject_awaiter(thrown_val);
                return true;
            }
            continue;
        };
        let f2 = ctx.frames.len() - 1;
        let b2 = ctx.frames[f2].base;
        let nregs = ctx.frames[f2].closure().proto.register_count as usize;
        ctx.stack.ensure_frame_size(b2, nregs);
        if ctx.stack.unbox_into_reg(b2, err_reg, thrown_val).is_err() {
            return false;
        }
        ctx.frames[f2].ip = catch_ip;
        return true;
    }
    false
}

pub(crate) unsafe fn raise(
    ctx: *mut crate::exec::ExecCtx,
    val: VmValue,
    depth: usize,
) -> Result<(), crate::error::RuntimeError> {
    let frames = collect_frames(unsafe { &(*ctx).frames });
    stamp_stack(unsafe { &mut (*ctx).heap }, val, &frames);
    let err = build_thrown_chain(val, unsafe { &(*ctx).heap }, frames, MAX_CAUSE_DEPTH);
    let thrown = err.thrown.unwrap_or(VmValue::null());
    if unsafe { dispatch_to_handler(ctx, thrown, depth) } {
        Ok(())
    } else {
        Err(err)
    }
}

pub(crate) unsafe fn handle_runtime_error(
    ctx: *mut crate::exec::ctx::ExecCtx,
    mut err: RuntimeError,
    depth: usize,
) -> crate::error::VmResult<()> {
    if err.frames.is_empty() {
        err.frames = collect_frames(unsafe { &(*ctx).frames });
    }
    let thrown = thrown_value_for(&err, unsafe { &mut (*ctx).heap });
    if unsafe { dispatch_to_handler(ctx, thrown, depth) } {
        Ok(())
    } else {
        Err(err)
    }
}
