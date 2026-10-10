use crate::closure::VmClosure;
use crate::exec::advanced::typeof_val;
use crate::exec::conform::{fit, mismatch};
use crate::exec::ctx::ExecCtx;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use varn_core::TypeDesc;
use varn_types::chunk::ContractWitness;
use varn_types::value::BoundMethodTarget;
use varn_types::{Boundary, ClassObj, NativeCtx, NativeError, NativeFn, NativeFnResult};

pub(crate) const RUNTIME_GLOBALS: [(&str, NativeFn); 7] = [
    (
        varn_core::abi::CONFORM_GLOBAL,
        crate::exec::conform::conform_native,
    ),
    (varn_core::abi::VERIFY_CALL_GLOBAL, verify_call_native),
    (varn_core::abi::VERIFY_SPREAD_GLOBAL, verify_spread_native),
    (varn_core::abi::VERIFY_METHOD_GLOBAL, verify_method_native),
    (
        varn_core::abi::VERIFY_METHOD_SPREAD_GLOBAL,
        verify_method_spread_native,
    ),
    (varn_core::abi::VERIFY_SUPER_GLOBAL, verify_super_native),
    (
        varn_core::abi::VERIFY_SUPER_SPREAD_GLOBAL,
        verify_super_spread_native,
    ),
];

fn spread_items(ctx: &dyn NativeCtx, flat: VmValue) -> Vec<VmValue> {
    (0..ctx.array_len(flat))
        .filter_map(|i| ctx.array_get(flat, i))
        .collect()
}

fn verify(ctx: &dyn NativeCtx, callee: VmValue, args: &[VmValue]) -> Result<(), NativeError> {
    match ctx.boundary() {
        Some(boundary) => boundary.verify_call(callee, args),
        None => Ok(()),
    }
}

fn method_of(ctx: &dyn NativeCtx, recv: VmValue, name: VmValue) -> VmValue {
    match ctx.boundary() {
        Some(boundary) => boundary.method_callee(recv, &ctx.str_repr_borrowed(name)),
        None => VmValue::null(),
    }
}

fn inherited_of(ctx: &dyn NativeCtx, class: VmValue, name: VmValue) -> VmValue {
    match ctx.boundary() {
        Some(boundary) => boundary.class_method(class, &ctx.str_repr_borrowed(name)),
        None => VmValue::null(),
    }
}

fn verify_call_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let [callee, rest @ ..] = args {
        verify(ctx, *callee, rest)?;
    }
    Ok(VmValue::null())
}

fn verify_spread_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let [callee, flat] = args {
        let items = spread_items(ctx, *flat);
        verify(ctx, *callee, &items)?;
    }
    Ok(VmValue::null())
}

fn verify_method_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let [recv, name, rest @ ..] = args {
        let callee = method_of(ctx, *recv, *name);
        verify(ctx, callee, rest)?;
    }
    Ok(VmValue::null())
}

fn verify_method_spread_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let [recv, name, flat] = args {
        let callee = method_of(ctx, *recv, *name);
        let items = spread_items(ctx, *flat);
        verify(ctx, callee, &items)?;
    }
    Ok(VmValue::null())
}

fn verify_super_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let [class, name, rest @ ..] = args {
        let callee = inherited_of(ctx, *class, *name);
        verify(ctx, callee, rest)?;
    }
    Ok(VmValue::null())
}

fn verify_super_spread_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let [class, name, flat] = args {
        let callee = inherited_of(ctx, *class, *name);
        let items = spread_items(ctx, *flat);
        verify(ctx, callee, &items)?;
    }
    Ok(VmValue::null())
}

pub(crate) fn class_method(ctx: &ExecCtx, class: VmValue, name: &str) -> VmValue {
    if class.is_heap() {
        if let Some(HeapObj::Class(class)) = ctx.heap.get(class.as_heap()) {
            return class.find_method(name).unwrap_or_else(VmValue::null);
        }
    }
    VmValue::null()
}

fn target_closure(heap: &Heap, callee: VmValue) -> Option<&Rc<VmClosure>> {
    if let Some(closure) = heap.closure_of(callee) {
        return Some(closure);
    }
    if !callee.is_heap() {
        return None;
    }
    if let Some(HeapObj::BoundMethod(bound)) = heap.get(callee.as_heap()) {
        return match &bound.target {
            BoundMethodTarget::Vm { closure, .. } => heap.closure_of(*closure),
            BoundMethodTarget::Native { .. } => None,
        };
    }
    if let Some(HeapObj::Class(class)) = heap.get(callee.as_heap()) {
        return class.constructor().and_then(|ctor| heap.closure_of(ctor));
    }
    None
}

pub(crate) fn method_callee(ctx: &ExecCtx, recv: VmValue, name: &str) -> VmValue {
    if let Some(inst) = ctx.heap.instance_of(recv) {
        return ClassObj::find_by_id(inst.class_id)
            .and_then(|class| class.find_method(name))
            .unwrap_or_else(VmValue::null);
    }
    if recv.is_heap() {
        if let Some(HeapObj::Class(class)) = ctx.heap.get(recv.as_heap()) {
            return class
                .statics
                .borrow()
                .get(name)
                .copied()
                .unwrap_or_else(VmValue::null);
        }
    }
    NativeCtx::get_field(ctx, recv, name).unwrap_or_else(VmValue::null)
}

fn resolve_witnesses(
    ctx: &ExecCtx,
    closure: &VmClosure,
    witnesses: &[ContractWitness],
) -> Vec<VmValue> {
    let globals = ctx.globals_ref();
    witnesses
        .iter()
        .map(|witness| {
            match witness {
                ContractWitness::Slot(slot) => {
                    globals.get_by_index(closure.module_base as usize + *slot as usize)
                }
                ContractWitness::Name(name) => globals.get_by_name(name),
            }
            .unwrap_or_else(VmValue::null)
        })
        .collect()
}

pub(crate) fn verify_call(
    ctx: &ExecCtx,
    callee: VmValue,
    args: &[VmValue],
) -> Result<(), NativeError> {
    let Some(closure) = target_closure(&ctx.heap, callee) else {
        return Ok(());
    };
    let proto = &closure.proto;
    let Some(contract) = &proto.param_contract else {
        return Ok(());
    };
    let Some(TypeDesc::Args { fixed, rest }) = proto
        .param_types
        .get_or_init(|| TypeDesc::decode(&contract.desc))
    else {
        return Ok(());
    };
    let witnesses = resolve_witnesses(ctx, closure, &contract.witnesses);
    for (i, arg) in args.iter().enumerate() {
        let Some(expected) = fixed.get(i).or(rest.as_deref()) else {
            break;
        };
        if fit(&ctx.heap, *arg, expected, &witnesses).is_none() {
            return Err(mismatch(format!(
                "type mismatch: argument {} expected '{expected}', got '{}'",
                i + 1,
                typeof_val(*arg, &ctx.heap)
            )));
        }
    }
    Ok(())
}

impl Boundary for ExecCtx {
    fn conform(&self, value: VmValue, desc: &str, witnesses: &[VmValue]) -> NativeFnResult {
        crate::exec::conform::conform(&self.heap, value, desc, witnesses)
    }

    fn verify_call(&self, callee: VmValue, args: &[VmValue]) -> Result<(), NativeError> {
        verify_call(self, callee, args)
    }

    fn method_callee(&self, recv: VmValue, name: &str) -> VmValue {
        method_callee(self, recv, name)
    }

    fn class_method(&self, class: VmValue, name: &str) -> VmValue {
        class_method(self, class, name)
    }
}
