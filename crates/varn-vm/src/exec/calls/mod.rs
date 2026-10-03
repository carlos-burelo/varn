use crate::closure::{VmClosure, VmUpvalue};
use crate::error::{RuntimeError, VmResult};
use crate::frame::CallFrame;
use crate::frame_store::FrameStore;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

use std::rc::Rc;
use varn_types::value::BoundMethodTarget;
use varn_types::{FunctionProto, Literal, PoolEntry, VmArray};

pub(crate) fn resolve_constants(proto: &FunctionProto, heap: &mut Heap) -> Vec<VmValue> {
    proto
        .chunk
        .constants
        .iter()
        .map(|entry| {
            let res = match entry {
                PoolEntry::Literal(lit) => match lit {
                    Literal::Null => VmValue::null(),
                    Literal::Bool(b) => VmValue::from_bool(*b),
                    Literal::Int(n) => VmValue::from_int(*n),
                    Literal::Float(f) => VmValue::from_f64(*f),
                    Literal::Str(s) => heap.alloc_str_interned(s.as_ref()),
                    Literal::BigInt(n) => heap.alloc_bigint(n.clone()),
                    Literal::Decimal(d) => heap.alloc_decimal(d.clone()),
                    Literal::Symbol(s) => heap.alloc_symbol(s.clone()),
                    Literal::Char(c) => heap.alloc_char(*c),
                },
                PoolEntry::Function(_) => VmValue::null(),
                PoolEntry::Shape(_) => VmValue::null(),
            };
            res
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

/// La ventana de llamada son los ÚLTIMOS `arg_count` valores de staging: los
/// caminos de método anteponen el `method_nv` fuera de la ventana (mismo
/// convenio del `stack.len() - arg_count` anterior).
#[inline]
fn stage_window(staging: &[VmValue], arg_count: usize) -> &[VmValue] {
    let start = staging.len().saturating_sub(arg_count);
    &staging[start..]
}

/// Materializa un frame VM adoptando la ventana de staging.
///
/// La ventana se adopta ALINEADA A LA IZQUIERDA y SOLO hasta `arity`: `r0` es
/// el callee/placeholder y `r1..` los args declarados en orden. Los registros
/// restantes son TEMPORALES del cuerpo (tipados por `register_meta`): no
/// adoptan valores de la ventana — un arg extra no declarado (los
/// `(item, index, array)` que `map` pasa a un callback de 1 parámetro) o el
/// valor de un temporal no puede acabar reinterpreto en un registro `Int`.
/// Quedan en su default de clase (los temps se escriben antes de leerse, y el
/// `null` == el padding del stack anterior).
///
/// La conversión a la clase de cada registro (ensanchado `int`→`float`
/// incluido) hace de un desajuste un `type mismatch`, nunca basura
/// reinterpretada.
pub(crate) fn materialize_frame(
    store: &mut FrameStore,
    nc: &Rc<VmClosure>,
    window: &[VmValue],
) -> VmResult<CallFrame> {
    let alloc = store.push_frame(&nc.proto);
    let nparams = nc.proto.arity.min(nc.proto.register_count as usize);
    for r in 0..nparams {
        let v = window.get(r).copied().unwrap_or_else(VmValue::null);
        if store.unbox_into_reg(alloc, r, v).is_err() {
            store.pop_frame();
            return Err(RuntimeError::new(format!(
                "type mismatch: argument {} does not fit parameter of '{}'",
                r,
                nc.proto.name.as_deref().unwrap_or("<anon>")
            )));
        }
    }
    Ok(CallFrame::new(nc, alloc))
}

pub enum PreparedCall {
    Frame(CallFrame),
    Constructor(CallFrame, VmValue),
    /// Native call whose arguments are read straight out of the register
    /// window rather than collected into a `Vec` — `arg_count` slots starting
    /// at the callee slot. The callee slot holds the RECEIVER, so the whole
    /// window is passed through: this is the bound-method form.
    NativeImmediate(varn_types::NativeFn, usize),
    /// As [`Self::NativeImmediate`], but for a bare native function, where the
    /// callee slot holds the callee itself rather than a receiver. The window
    /// is passed minus that first slot. Collapsing the two hands every bare
    /// native its own function as `args[0]` and shifts every real argument by
    /// one — see `varn-builtins`, which indexes arguments from 0.
    RawNativeImmediate(varn_types::NativeFn, usize),
    NativeConstructor(varn_types::NativeFn, Vec<VmValue>, VmValue),
    PushValue(VmValue),
    /// A generator body, described but not yet built.
    ///
    /// Building it needs a whole `ExecCtx` of its own, and that context must be
    /// a FORK of the one making the call — same globals above all. Global
    /// access is rewritten to `LoadGlobalIdx <slot>` against ONE store (see
    /// `crate::globals::resolve`), so a generator handed a fresh empty store
    /// reads slot indices into an empty vector: `function* g() { yield f() }`
    /// died with "value is not callable: 0" for any `f` the inliner had not
    /// already folded away.
    ///
    /// `prepare_call` cannot fork — it holds `stack` and `heap` split off the
    /// context precisely so it does not need it — so it describes the
    /// generator and [`ExecCtx::dispatch_prepared_call`] materialises it.
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
