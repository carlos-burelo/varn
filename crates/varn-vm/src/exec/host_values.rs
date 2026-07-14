//! Materializes, on the consumer's heap, the marker/envelope values that
//! builtins produce without access to the destination heap (same-thread and,
//! for isolates, cross-thread):
//!
//! - [`SendEnvelope`] → the carried [`SendValue`] materialized via
//!   `to_value_ctx` (optionally re-wrapped as `{value, done:false}` for the
//!   `for await` protocol). Keeps composite channel payloads off the producer's
//!   GC heap.
//! - `{__chanEndpoint: "tx"|"rx", __chanId}` → a real `Sender`/`Receiver`
//!   instance.
//! - [`HostError`] (or a `{__hostErrorClass, message}` marker object, for
//!   producers that can pre-intern strings) → a real instance of that
//!   intrinsic class (so `instanceof` works inside the `catch`).
//!
//! Single application point: await-resume (`ctx_tasks`).

use varn_types::value::{nv_to_value, HostError, SendEnvelope};
use varn_types::{NativeCtx, Value};

use super::ctx::ExecCtx;
use crate::value::VmValue;

fn marker_str(val: &Value, key: &str) -> Option<String> {
    if let Value::Object(o) = val {
        if let Some(Value::Str(s)) = o.read().get(key).map(nv_to_value) {
            return Some(s.to_string());
        }
    }
    None
}

fn marker_int(val: &Value, key: &str) -> Option<i64> {
    if let Value::Object(o) = val {
        if let Some(Value::Int(i)) = o.read().get(key).map(nv_to_value) {
            return Some(i);
        }
    }
    None
}

/// Resolved values: materialize send-envelopes and mint endpoint markers.
pub fn open_resolved(ctx: &mut ExecCtx, val: Value) -> Value {
    // Composite channel payload carried heap-independently.
    if let Some(env) = SendEnvelope::from_value(&val) {
        let sv = env.sv.clone();
        let wrap = env.wrap;
        let val_nv = sv.to_value_ctx(ctx);
        if wrap {
            // `Receiver::next` / `for await`: deliver `{value, done:false}`.
            let obj = ctx.alloc_object();
            ctx.set_field(obj, "value", val_nv);
            let done_nv = ctx.bool_val(false);
            ctx.set_field(obj, "done", done_nv);
            return ctx.heap.extract(obj);
        }
        return ctx.heap.extract(val_nv);
    }

    // Channel endpoint crossing into this heap.
    if let Some(dir) = marker_str(&val, "__chanEndpoint") {
        if let Some(id) = marker_int(&val, "__chanId") {
            let class_name = if dir == "tx" { "Sender" } else { "Receiver" };
            if let Some(nv) = mint_endpoint(ctx, class_name, id as u64) {
                return ctx.heap.extract(nv);
            }
        }
    }

    val
}

/// Rejected values: turn typed [`HostError`] payloads (and `{__hostErrorClass,
/// message}` marker objects, for producers that can pre-intern strings) into
/// real instances of the named intrinsic class so
/// `catch (e) { e instanceof X }` works.
pub fn open_rejected(ctx: &mut ExecCtx, val: Value) -> Value {
    let (class_name, msg) = if let Some(he) = HostError::from_value(&val) {
        (he.class.clone(), he.message.clone())
    } else if let Some(class_name) = marker_str(&val, "__hostErrorClass") {
        let msg = marker_str(&val, "message").unwrap_or_default();
        (class_name, msg)
    } else {
        return val;
    };
    if let Some(inst_nv) = ctx.alloc_instance(&class_name) {
        let msg_nv = ctx.alloc_str(&msg);
        ctx.set_field(inst_nv, "message", msg_nv);
        let name_nv = ctx.alloc_str(&class_name);
        ctx.set_field(inst_nv, "name", name_nv);
        return ctx.heap.extract(inst_nv);
    }
    val
}

/// Mint a `Sender`/`Receiver` on this heap. Delegates to the builtins helper so
/// the endpoint layout (including the receiver's `Symbol.asyncIterator`) has a
/// single definition shared with `channel()`'s own construction path.
fn mint_endpoint(ctx: &mut ExecCtx, class_name: &str, id: u64) -> Option<VmValue> {
    varn_builtins::modules::task::alloc_endpoint(ctx, class_name, id).ok()
}
