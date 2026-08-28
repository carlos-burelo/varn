use crate::closure::{VmClosure, VmClosurePayload, VmUpvalue};
use crate::error::{RuntimeError, VmResult};
use crate::frame::CallFrame;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

use std::rc::Rc;
use varn_types::value::BoundMethodTarget;
use varn_types::value::LazyTask;
use varn_types::{FunctionProto, Literal, PoolEntry, Value, VmArray};

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
                    Literal::Int(n) => {
                        const I48_MIN: i64 = -(1_i64 << 47);
                        const I48_MAX: i64 = (1_i64 << 47) - 1;
                        if *n >= I48_MIN && *n <= I48_MAX {
                            VmValue::from_int(*n)
                        } else {
                            VmValue::from_f64(*n as f64)
                        }
                    }
                    Literal::Float(f) => VmValue::from_f64(*f),
                    Literal::Str(s) => heap.alloc_str_interned(s.as_ref()),
                    Literal::BigInt(n) => heap.intern(Value::BigInt(Box::new(*n))),
                    Literal::Decimal(d) => heap.intern(Value::Decimal(Box::new(*d))),
                    Literal::Symbol(s) => heap.intern(Value::Symbol(s.clone())),
                    Literal::Char(c) => heap.intern(Value::Char(*c)),
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

#[inline(always)]
pub(crate) fn try_prepare_call_fast(
    callee_nv: VmValue,
    arg_count: usize,
    stack: &[VmValue],
    heap: &Heap,
) -> Option<(PreparedCall, bool)> {
    if !callee_nv.is_heap() {
        return None;
    }

    match heap.get(callee_nv.as_heap_idx())? {
        HeapObj::VmClosure(nc) => {
            if !nc.proto.is_generator && !nc.proto.is_async
                && (!nc.proto.has_rest || arg_count <= nc.proto.arity) {
                    let base = stack.len() - arg_count;
                    return Some((PreparedCall::Frame(CallFrame::new(nc, base)), false));
                }
            None
        }
        // A bound method is the hot shape for every stdlib method call
        // (`arr.push`, `Map.get`, …). Dropping it to the slow `prepare_call`
        // turns 21k immediate native returns into 10k frame pushes — measured
        // 3.8x on tests/main.vn — so both targets keep their fast path. The
        // receiver fills the staged callee slot (`stack[frame.base]`) in
        // `ExecCtx::prepare_call`, which is why the flag comes back `true`.
        // Only the NATIVE target. It is the hot shape for every stdlib method
        // call (`arr.push`, `Map.get`, …) and returns immediately with no frame
        // — dropping it to the slow `prepare_call` turned 21k immediate native
        // returns into 10k frame pushes, measured 3.8x on tests/main.vn.
        //
        // The VM target deliberately stays slow: staging a `CallFrame` here
        // skips the receiver bookkeeping that `super.method()` needs and fails
        // tests/48-opt-unsupported-phase1.vn with "super write via method".
        //
        // The receiver fills the staged callee slot in `ExecCtx::prepare_call`,
        // which is why the flag comes back `true`.
        HeapObj::BoundMethod(bm) => match &bm.target {
            BoundMethodTarget::Native { func, .. } => {
                Some((PreparedCall::NativeImmediate(*func, arg_count), true))
            }
            BoundMethodTarget::Vm { .. } => None,
        },
        HeapObj::NativeFn(f, _name) => {
            Some((PreparedCall::RawNativeImmediate(*f, arg_count), false))
        }
        _ => None,
    }
}

/// Take the generator's arguments off the stack and package its closure, so
/// the caller's context can build the generator's own context by forking
/// itself. Shared by the bare-closure and bound-method paths — writing it
/// twice is what let the empty-globals bug live in both.
fn describe_generator(
    nc: &Rc<VmClosure>,
    arg_count: usize,
    current_class: Option<Rc<varn_types::ClassObj>>,
    stack: &mut Vec<VmValue>,
    heap: &mut Heap,
    settings: crate::settings::ExecSettings,
) -> PreparedCall {
    let args_start = stack.len() - arg_count;
    let args: Vec<VmValue> = stack.drain(args_start..).collect();
    let constants = resolve_constants(&nc.proto, heap);
    // Read AFTER the drain: an upvalue points at a stack location, and the
    // argument window is gone by now.
    let upvalues = nc
        .upvalues
        .iter()
        .map(|uv| VmUpvalue::closed(uv.read(stack)))
        .collect();
    PreparedCall::Generator {
        closure: Rc::new(VmClosure::with_upvalues(
            nc.proto.clone(),
            upvalues,
            Rc::new(constants),
            settings,
        )),
        args,
        current_class,
    }
}

pub(crate) fn prepare_call(
    callee_nv: VmValue,
    arg_count: usize,
    stack: &mut Vec<VmValue>,
    heap: &mut Heap,
    settings: crate::settings::ExecSettings,
) -> VmResult<PreparedCall> {
    let mut arg_count = arg_count;

    if callee_nv.is_heap() {
        match heap
            .get(callee_nv.as_heap_idx())
            .expect("invalid heap index")
        {
            HeapObj::VmClosure(nc) => {
                let nc = nc.clone();
                bundle_rest_args(&nc.proto, &mut arg_count, stack, heap);
                if nc.proto.is_generator {
                    return Ok(describe_generator(
                        &nc, arg_count, None, stack, heap, settings,
                    ));
                }
                if nc.proto.is_async {
                    let args_start = stack.len() - arg_count;
                    let args: Vec<Value> = stack
                        .drain(args_start..)
                        .map(|nv| heap.extract(nv))
                        .collect();
                    let upvalues: Vec<varn_types::Upvalue> = nc
                        .upvalues
                        .iter()
                        .map(|uv| {
                            let nv = uv.read(stack);
                            let val = heap.extract(nv);
                            varn_types::Upvalue {
                                inner: std::rc::Rc::new(std::cell::RefCell::new(
                                    varn_types::UpvalueInner {
                                        value: val,
                                        location: None,
                                    },
                                )),
                            }
                        })
                        .collect();
                    let consts: Vec<varn_types::Value> =
                        nc.constants.iter().map(|&c| heap.extract(c)).collect();
                    let closure = varn_types::Closure::new(nc.proto.clone(), upvalues, consts);
                    let task = Value::Task(std::rc::Rc::new(LazyTask {
                        closure: std::rc::Rc::new(closure),
                        args,
                        current_class: None,
                    }));

                    return Ok(PreparedCall::PushValue(heap.intern(task)));
                }
                let base = stack.len() - nc.proto.arity;
                return Ok(PreparedCall::Frame(CallFrame::new(&nc, base)));
            }
            HeapObj::NativeFn(f, _name) => {
                let func = *f;
                return Ok(PreparedCall::RawNativeImmediate(func, arg_count));
            }
            HeapObj::BoundMethod(bm) => {
                let bm = bm.clone();
                match bm.target {
                    BoundMethodTarget::Native { func, .. } => {
                        let recv_nv = heap.intern(bm.receiver);
                        let args_start = stack.len() - arg_count;
                        let mut final_count = arg_count;
                        if args_start >= stack.len() {
                            stack.push(recv_nv);
                            final_count = 1;
                        } else {
                            stack[args_start] = recv_nv;
                        }
                        return Ok(PreparedCall::NativeImmediate(func, final_count));
                    }
                    BoundMethodTarget::Vm {
                        closure,
                        owner_class,
                    } => {
                        let recv_nv = heap.intern(bm.receiver);
                        let nc = if let Some(wrapper) =
                            closure.as_any().downcast_ref::<VmClosurePayload>()
                        {
                            wrapper.0.clone()
                        } else {
                            return Err(RuntimeError::new(
                                "BoundMethod(Vm): invalid closure payload",
                            ));
                        };
                        // `arity` already counts register 0 — the callee slot the
                        // caller stages as a null placeholder — plus the declared
                        // params, so the receiver FILLS that slot rather than
                        // shifting it. A caller that staged only the args (no
                        // placeholder) is one short, and gets the receiver
                        // inserted in front instead.
                        let mut full_arg_count = arg_count;
                        let base = stack.len() - arg_count;
                        if base >= stack.len() {
                            stack.push(recv_nv);
                            full_arg_count = 1;
                        } else {
                            stack[base] = recv_nv;
                        }
                        if nc.proto.is_generator {
                            return Ok(describe_generator(
                                &nc,
                                full_arg_count,
                                owner_class,
                                stack,
                                heap,
                                settings,
                            ));
                        }
                        if nc.proto.is_async {
                            let args_start = stack.len() - full_arg_count;
                            let args: Vec<Value> = stack
                                .drain(args_start..)
                                .map(|nv| heap.extract(nv))
                                .collect();
                            let upvalues: Vec<varn_types::Upvalue> = nc
                                .upvalues
                                .iter()
                                .map(|uv| {
                                    let nv = uv.read(stack);
                                    let val = heap.extract(nv);
                                    varn_types::Upvalue {
                                        inner: std::rc::Rc::new(std::cell::RefCell::new(
                                            varn_types::UpvalueInner {
                                                value: val,
                                                location: None,
                                            },
                                        )),
                                    }
                                })
                                .collect();
                            let consts: Vec<varn_types::Value> =
                                nc.constants.iter().map(|&c| heap.extract(c)).collect();
                            let closure =
                                varn_types::Closure::new(nc.proto.clone(), upvalues, consts);
                            let task = Value::Task(std::rc::Rc::new(LazyTask {
                                closure: std::rc::Rc::new(closure),
                                args,
                                current_class: owner_class,
                            }));

                            return Ok(PreparedCall::PushValue(heap.intern(task)));
                        }
                        if !nc.proto.is_generator && !nc.proto.is_async {
                            bundle_rest_args(&nc.proto, &mut full_arg_count, stack, heap);
                            let final_base = stack.len() - full_arg_count;
                            let mut frame = CallFrame::new(&nc, final_base);
                            frame.current_class = owner_class;
                            if nc.proto.name.as_deref() == Some("constructor") {
                                return Ok(PreparedCall::Constructor(frame, recv_nv));
                            }
                            return Ok(PreparedCall::Frame(frame));
                        }
                        return Err(RuntimeError::new("BoundMethod(Vm): invalid VmClosure"));
                    }
                }
            }
            HeapObj::Class(cls) => {
                let cls = cls.clone();
                let oref = varn_types::value::ObjRef::instance(cls.clone());
                let instance_nv = VmValue::from_heap_idx(heap.alloc(HeapObj::Object(oref)));
                if let Some(ctor) = cls.constructor() {
                    let mut full_arg_count = arg_count;
                    let base = stack.len() - arg_count;
                    if base >= stack.len() {
                        stack.push(instance_nv);
                        full_arg_count = 1;
                    } else {
                        stack[base] = instance_nv;
                    }
                    match ctor {
                        Value::VmValue(payload) => {
                            if let Some(wrapper) =
                                payload.as_any().downcast_ref::<VmClosurePayload>()
                            {
                                let nc = wrapper.0.clone();
                                bundle_rest_args(&nc.proto, &mut full_arg_count, stack, heap);
                                let final_base = stack.len() - full_arg_count;
                                let mut frame = CallFrame::new_owned(nc, final_base);
                                frame.current_class = Some(cls.clone());
                                return Ok(PreparedCall::Constructor(frame, instance_nv));
                            }
                        }
                        Value::NativeFn(b) => {
                            let (f, _) = *b;

                            let vm_args: Vec<VmValue> = stack.drain(base..).collect();
                            return Ok(PreparedCall::NativeConstructor(f, vm_args, instance_nv));
                        }
                        _ => {}
                    }
                }
                let args_start = stack.len() - arg_count;
                stack.drain(args_start..);
                return Ok(PreparedCall::PushValue(instance_nv));
            }
            HeapObj::EnumVariant(data) => {
                let data = data.clone();
                let args_start = stack.len() - arg_count;
                let mut args: Vec<VmValue> = stack.drain(args_start..).collect();

                if !args.is_empty() {
                    args.remove(0);
                }

                if data.fields.is_empty() && args.is_empty() {
                    return Ok(PreparedCall::PushValue(callee_nv));
                }

                let payload = if !data.fields.is_empty() {
                    Value::Object(varn_types::value::ObjRef::from_pairs(
                        data.fields.iter().enumerate().map(|(idx, field_name)| {
                            let nv = args.get(idx).copied().unwrap_or(VmValue::null());
                            (field_name.clone(), nv)
                        }),
                    ))
                } else if args.len() == 1 {
                    heap.extract(args[0])
                } else if args.len() > 1 {
                    Value::Array(varn_types::value::ArrayRef::new(
                        args.iter().map(|&nv| heap.extract(nv)).collect(),
                    ))
                } else {
                    Value::Null
                };

                let mut new_data = *data;
                new_data.payload = payload;
                return Ok(PreparedCall::PushValue(VmValue::from_heap_idx(
                    heap.alloc(HeapObj::EnumVariant(Box::new(new_data))),
                )));
            }
            _ => {}
        }
    }

    let callee_repr = heap.str_repr(callee_nv);
    let extracted = heap.extract(callee_nv);
    let type_name = match extracted {
        Value::Class(ref c) => c.name.as_str(),
        ref other => other.type_name(),
    };
    if callee_nv.is_null() {
        Err(RuntimeError::new(
            "Cannot invoke function because the value is null. Check if the function or host entry exists and is exported.",
        ))
    } else {
        Err(RuntimeError::new(format!(
            "value is not callable: {callee_repr} (type: {type_name})",
        )))
    }
}

pub(crate) fn bundle_rest_args(
    proto: &FunctionProto,
    arg_count: &mut usize,
    stack: &mut Vec<VmValue>,
    heap: &mut Heap,
) {
    let arity = proto.arity;
    if proto.has_rest {
        let rest_idx = arity.saturating_sub(1);
        if *arg_count > rest_idx {
            let num_to_bundle = *arg_count - rest_idx;
            let start = stack.len() - num_to_bundle;
            let items: Vec<VmValue> = stack.drain(start..).collect();
            let va = VmArray::new(items);
            let nv = VmValue::from_heap_idx(heap.alloc(crate::heap::HeapObj::Array(va)));
            stack.push(nv);
            *arg_count = rest_idx + 1;
        } else {
            for _ in *arg_count..rest_idx {
                stack.push(VmValue::null());
            }
            let aref = VmArray::new(vec![]);
            let nv = VmValue::from_heap_idx(heap.alloc(crate::heap::HeapObj::Array(aref)));
            stack.push(nv);
            *arg_count = rest_idx + 1;
        }
    } else if *arg_count < arity {
        for _ in *arg_count..arity {
            stack.push(VmValue::null());
        }
        *arg_count = arity;
    }
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
