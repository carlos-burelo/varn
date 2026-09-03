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
        .map(|f| {
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
            FrameInfo {
                fn_name,
                file,
                line,
            }
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

/// El valor que verá el `catch`.
///
/// Un `throw` del usuario ya trae el suyo. Un error nacido en el runtime
/// —división por cero, una nativa que falla— no traía ninguno porque nunca
/// llegaba a un `catch`: la búsqueda de handler vivía dentro del brazo del
/// opcode `Throw`, así que todo `Err(RuntimeError)` salía del bucle sin mirar
/// la tabla de excepciones. Se materializa como una instancia de `Error` para
/// que `e.message` funcione igual que con uno lanzado a mano.
pub(crate) fn thrown_value_for(err: &RuntimeError, heap: &mut Heap) -> VmValue {
    if let Some(v) = err.thrown {
        return v;
    }
    let msg = heap.alloc_str_dynamic(&err.message);
    let Some(cls) = heap.get_intrinsic_class(IntrinsicType::Error.as_str()) else {
        // Sin la clase `Error` registrada (arranque temprano, isolate sin
        // globals): el mensaje suelto sigue siendo capturable e imprimible.
        return msg;
    };
    let oref = varn_types::value::ObjRef::instance(cls);
    oref.set_field_nv(std::rc::Rc::from("message"), msg);
    VmValue::from_heap_idx(heap.alloc(HeapObj::Object(oref)))
}

/// Busca un handler para `thrown_val` y, si lo encuentra, deja el contexto
/// listo para reanudar dentro del `catch`. Devuelve si lo manejó.
///
/// `depth` es el fondo de la invocación actual de la máquina: por debajo hay
/// frames de un llamador de Rust, que no puede reanudarse desde aquí, así que
/// el error tiene que propagarse como `Err` en vez de desmontarlos.
#[allow(dangerous_implicit_autorefs)]
pub(crate) unsafe fn dispatch_to_handler(
    ctx: *mut crate::exec::ExecCtx,
    thrown_val: VmValue,
    depth: usize,
) -> bool {
    if let Some(handler) = (*ctx).try_handlers.pop_if(|h| h.frame_depth > depth) {
        crate::exec::frame_ctrl::unwind_to_handler(&mut *ctx, handler, thrown_val);
        return true;
    }
    // Tabla lateral: coste cero mientras no se lanza nada.
    while (*ctx).frames.len() > depth {
        let cur_ip = (*ctx).frames.last().unwrap().ip as u32;
        let proto = &(*ctx).frames.last().unwrap().closure().proto;
        let hit = proto
            .exception_table
            .iter()
            .find(|r| cur_ip >= r.try_start_ip && cur_ip <= r.try_end_ip)
            .map(|r| (r.catch_ip as usize, r.err_reg as usize));
        let Some((catch_ip, err_reg)) = hit else {
            let popped = (*ctx).frames.pop().unwrap();
            (*ctx).close_upvalues_above(popped.base);
            continue;
        };
        let f2 = (*ctx).frames.len() - 1;
        let b2 = (*ctx).frames[f2].base;
        let required_depth = b2 + (*ctx).frames[f2].closure().proto.register_count as usize;
        (*ctx).stack.truncate(required_depth);
        let slot = b2 + err_reg;
        if slot >= (*ctx).stack.len() {
            (*ctx).stack.resize(slot + 1, VmValue::null());
        }
        (*ctx).stack[slot] = thrown_val;
        (*ctx).frames[f2].ip = catch_ip;
        return true;
    }
    false
}
