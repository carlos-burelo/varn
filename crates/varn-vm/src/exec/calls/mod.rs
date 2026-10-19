use crate::closure::{VmClosure, VmUpvalue};
use crate::error::{RuntimeError, VmResult};
use crate::frame::CallFrame;
use crate::frame_store::FrameStore;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

use std::rc::Rc;
use varn_types::value::BoundMethodTarget;
use varn_types::{FunctionProto, Literal, PoolEntry};

pub(crate) fn resolve_constants(proto: &FunctionProto, heap: &mut Heap) -> Vec<VmValue> {
    proto
        .chunk
        .constants
        .iter()
        .map(|entry| match entry {
            PoolEntry::Literal(lit) => match lit {
                Literal::Null => VmValue::null(),
                Literal::Bool(b) => VmValue::from_bool(*b),
                Literal::Int(n) => VmValue::from_int(*n),
                Literal::Float(f) => VmValue::from_f64(*f),
                Literal::Str(s) => heap.alloc_str_interned(s.as_ref()),
                Literal::BigInt(n) => heap.alloc_bigint(n.clone()),
                Literal::Decimal(d) => heap.alloc_decimal(d.clone()),
                Literal::Char(c) => heap.alloc_char(*c),
            },
            PoolEntry::Function(_) => VmValue::null(),
            PoolEntry::Shape(_) => VmValue::null(),
            PoolEntry::Layout(_) | PoolEntry::Variant(_) => VmValue::null(),
        })
        .collect()
}

pub(crate) fn build_closure(
    proto: Rc<FunctionProto>,
    heap: &mut Heap,
    settings: crate::settings::ExecSettings,
) -> Rc<VmClosure> {
    let constants = resolve_constants(&proto, heap);
    Rc::new(VmClosure::new(proto, constants, settings))
}

#[inline]
fn stage_window(staging: &[VmValue], arg_count: usize) -> &[VmValue] {
    let start = staging.len().saturating_sub(arg_count);
    &staging[start..]
}

pub(crate) fn materialize_frame(
    store: &mut FrameStore,
    nc: &Rc<VmClosure>,
    window: &[VmValue],
) -> VmResult<CallFrame> {
    let alloc = store.push_frame(&nc.proto);
    let nparams = nc.proto.arity.min(nc.proto.register_count as usize);
    for r in 0..nparams {
        let v = window.get(r).copied().unwrap_or_else(VmValue::null);
        if let Err(mut rejected) = store.unbox_into_reg(alloc, r, v) {
            store.pop_frame();
            rejected.message = rejected.message.replacen(
                "type mismatch: ",
                &format!("type mismatch: argument {r} "),
                1,
            );
            return Err(rejected);
        }
    }
    Ok(CallFrame::new(nc, alloc))
}

pub struct LazyParts {
    pub proto: Rc<FunctionProto>,
    pub upvalues: Vec<VmValue>,
    pub module_base: u32,
    pub args: Vec<VmValue>,
    pub current_class: Option<Rc<varn_types::ClassObj>>,
}

pub enum PreparedCall {
    Frame(CallFrame),

    Async(LazyParts),
    Constructor(CallFrame, VmValue),

    NativeImmediate(varn_types::NativeFn, usize),

    RawNativeImmediate(varn_types::NativeFn, usize),
    NativeConstructor(varn_types::NativeFn, Vec<VmValue>, VmValue),
    PushValue(VmValue),

    Generator {
        closure: Rc<VmClosure>,
        args: Vec<VmValue>,
        current_class: Option<Rc<varn_types::ClassObj>>,
    },
}

mod args;
mod prepare;

pub(crate) use args::{bundle_rest_args, expand_spread_args};
pub(crate) use prepare::{prepare_call, try_prepare_call_fast};
