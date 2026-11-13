use super::contract_natives::{
    verify_call_native, verify_method_native, verify_method_spread_native, verify_spread_native,
    verify_super_native, verify_super_spread_native,
};
use crate::closure::VmClosure;
use crate::exec::advanced::typeof_val;
use crate::exec::conform::{fit, mismatch};
use crate::exec::ctx::ExecCtx;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use vn_core::TypeDesc;
use vn_types::chunk::ContractWitness;
use vn_types::value::BoundMethodTarget;
use vn_types::{Boundary, ClassObj, NativeCtx, NativeError, NativeFn, NativeFnResult};

const RUNTIME_OPS: [(&str, NativeFn); vn_core::runtime_ops::ALL.len()] = [
    (
        vn_core::runtime_ops::CONFORM,
        crate::exec::conform::conform_native,
    ),
    (
        vn_core::runtime_ops::DECODE,
        crate::exec::conform::decode_native,
    ),
    (vn_core::runtime_ops::VERIFY_CALL, verify_call_native),
    (vn_core::runtime_ops::VERIFY_SPREAD, verify_spread_native),
    (vn_core::runtime_ops::VERIFY_METHOD, verify_method_native),
    (
        vn_core::runtime_ops::VERIFY_METHOD_SPREAD,
        verify_method_spread_native,
    ),
    (vn_core::runtime_ops::VERIFY_SUPER, verify_super_native),
    (
        vn_core::runtime_ops::VERIFY_SUPER_SPREAD,
        verify_super_spread_native,
    ),
    (vn_core::runtime_ops::STR_INDEX, str_index_native),
    (vn_core::runtime_ops::INHERITED, inherited_native),
    (
        vn_core::runtime_ops::PARSE_TYPED,
        crate::exec::conform::parse_typed_native,
    ),
];

fn inherited_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let [home, receiver, name, accessor] = args else {
        return Ok(VmValue::null());
    };
    let name = ctx.str_repr(*name);
    let accessor = accessor.is_truthy();
    match ctx.boundary_mut() {
        Some(boundary) => boundary.inherited(*home, *receiver, &name, accessor),
        None => Ok(VmValue::null()),
    }
}

fn str_index_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let [text, index] = args else {
        return Ok(VmValue::null());
    };
    let at = ctx.as_int(*index);
    let found = ctx.char_at(*text, at);
    if found.is_null() {
        return Err(vn_types::NativeError::rethrow(
            vn_core::RuntimeErrorKind::RangeError,
            format!(
                "index {at} is out of range for length {}",
                ctx.str_repr(*text).chars().count()
            ),
            None,
        ));
    }
    Ok(found)
}

pub(crate) fn runtime_op(op_id: u64) -> Option<NativeFn> {
    static BY_ID: std::sync::OnceLock<[NativeFn; vn_core::runtime_ops::ALL.len()]> =
        std::sync::OnceLock::new();
    let by_id = BY_ID.get_or_init(|| {
        vn_core::runtime_ops::ALL.map(|name| {
            RUNTIME_OPS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, f)| *f)
                .unwrap_or_else(|| panic!("VM runtime op `{name}` has no implementation"))
        })
    });
    by_id.get(usize::try_from(op_id).ok()?).copied()
}

pub(crate) fn native_op(op_id: u64) -> Option<NativeFn> {
    runtime_op(op_id).or_else(|| vn_builtins::native_op_fn(op_id))
}

pub(crate) fn class_method(ctx: &ExecCtx, class: VmValue, name: &str) -> VmValue {
    if class.is_heap()
        && let Some(HeapObj::Class(class)) = ctx.heap.get(class.as_heap())
    {
        return class.find_method(name).unwrap_or_else(VmValue::null);
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
    if recv.is_heap()
        && let Some(HeapObj::Class(class)) = ctx.heap.get(recv.as_heap())
    {
        return class
            .statics
            .borrow()
            .get(name)
            .copied()
            .unwrap_or_else(VmValue::null);
    }
    NativeCtx::get_field(ctx, recv, name).unwrap_or_else(VmValue::null)
}

const INLINE_WITNESSES: usize = 4;

struct Witnesses {
    inline: [VmValue; INLINE_WITNESSES],
    len: usize,
    spilled: Vec<VmValue>,
}

impl Witnesses {
    fn as_slice(&self) -> &[VmValue] {
        if self.spilled.is_empty() {
            &self.inline[..self.len]
        } else {
            &self.spilled
        }
    }
}

fn resolve_witnesses(
    ctx: &ExecCtx,
    closure: &VmClosure,
    witnesses: &[ContractWitness],
) -> Witnesses {
    let globals = ctx.globals_ref();
    let resolve = |witness: &ContractWitness| {
        match witness {
            ContractWitness::Slot(slot) => {
                globals.get_by_index(closure.module_base as usize + *slot as usize)
            }
            ContractWitness::Name(name) => globals.get_by_name(name),
        }
        .unwrap_or_else(VmValue::null)
    };
    let mut resolved = Witnesses {
        inline: [VmValue::null(); INLINE_WITNESSES],
        len: witnesses.len().min(INLINE_WITNESSES),
        spilled: Vec::new(),
    };
    if witnesses.len() > INLINE_WITNESSES {
        resolved.spilled = witnesses.iter().map(resolve).collect();
    } else {
        for (slot, witness) in witnesses.iter().enumerate() {
            resolved.inline[slot] = resolve(witness);
        }
    }
    resolved
}

fn check_args(
    heap: &crate::heap::Heap,
    stack_floor: u64,
    args: &[VmValue],
    given: &[VmValue],
    fixed: &[TypeDesc],
    rest: Option<&TypeDesc>,
    witnesses: &[VmValue],
) -> Result<(), NativeError> {
    for (i, arg) in args.iter().enumerate() {
        let Some(expected) = fixed.get(i).or(rest) else {
            break;
        };
        let scalar_float = matches!(expected, TypeDesc::Float)
            || matches!(expected, TypeDesc::Nullable(inner) if matches!(**inner, TypeDesc::Float));
        if !(scalar_float && arg.is_int())
            && fit(heap, stack_floor, *arg, expected, witnesses).is_none()
        {
            return Err(mismatch(format!(
                "type mismatch: argument {} expected '{expected}', got '{}'",
                i + 1,
                typeof_val(given.get(i).copied().unwrap_or(*arg), heap)
            )));
        }
    }
    Ok(())
}

#[inline]
pub(crate) fn has_contract(ctx: &ExecCtx, callee: VmValue) -> bool {
    target_closure(&ctx.heap, callee).is_some_and(|closure| closure.proto.checks_params)
}

pub(crate) fn verify_call(
    ctx: &mut ExecCtx,
    callee: VmValue,
    args: &[VmValue],
) -> Result<(), NativeError> {
    let Some(closure) =
        target_closure(&ctx.heap, callee).filter(|closure| closure.proto.checks_params)
    else {
        return Ok(());
    };
    let proto = &closure.proto;
    let Some(contract) = &proto.param_contract else {
        return Ok(());
    };
    let Some(desc) = proto
        .param_types
        .get_or_init(|| TypeDesc::decode(&contract.desc))
    else {
        return Ok(());
    };
    let TypeDesc::Args { fixed, rest } = desc else {
        return Ok(());
    };
    let witnesses = resolve_witnesses(ctx, closure, &contract.witnesses);
    check_args(
        &ctx.heap,
        ctx.jit_stack_floor,
        args,
        args,
        fixed,
        rest.as_deref(),
        witnesses.as_slice(),
    )?;
    Ok(())
}

impl Boundary for ExecCtx {
    fn conform(&mut self, value: VmValue, desc: &str, witnesses: &[VmValue]) -> NativeFnResult {
        crate::exec::conform::conform(&mut self.heap, self.jit_stack_floor, value, desc, witnesses)
    }

    fn decode(&mut self, value: VmValue, desc: &str, witnesses: &[VmValue]) -> NativeFnResult {
        crate::exec::conform::decode(&mut self.heap, self.jit_stack_floor, value, desc, witnesses)
    }

    fn parse_typed(
        &mut self,
        text: VmValue,
        desc: &str,
        witnesses: &[VmValue],
    ) -> NativeFnResult {
        crate::exec::conform::parse_typed(self, text, desc, witnesses)
    }

    fn verify_call(&mut self, callee: VmValue, args: &[VmValue]) -> Result<(), NativeError> {
        verify_call(self, callee, args)
    }

    fn method_callee(&self, recv: VmValue, name: &str) -> VmValue {
        method_callee(self, recv, name)
    }

    fn class_method(&self, class: VmValue, name: &str) -> VmValue {
        class_method(self, class, name)
    }

    fn inherited(
        &mut self,
        home: VmValue,
        receiver: VmValue,
        name: &str,
        accessor: bool,
    ) -> NativeFnResult {
        let parent = match self.heap.heap_ref(home).and_then(|at| self.heap.get(at)) {
            Some(HeapObj::Class(class)) => class.superclass.borrow().clone(),
            Some(
                HeapObj::Str(_)
                | HeapObj::Array(_)
                | HeapObj::Tuple(_)
                | HeapObj::Object(_)
                | HeapObj::Buffer(_)
                | HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmClosure(_)
                | HeapObj::NativeFn(..)
                | HeapObj::BoundMethod(_)
                | HeapObj::Map(_)
                | HeapObj::Set(_)
                | HeapObj::Task(_)
                | HeapObj::TaskHandle(_)
                | HeapObj::Range(_)
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::EnumVariant(_),
            )
            | None => None,
        };
        let found = parent.as_ref().and_then(|class| {
            if accessor {
                class.find_accessor(vn_core::AccessorKind::Get, name)
            } else {
                class.find_method(name)
            }
        });
        match found {
            Some(member) => Ok(crate::exec::props::bind_method_to_receiver(
                &mut self.heap,
                receiver,
                member,
                parent,
            )),
            None => Err(NativeError::rethrow(
                vn_core::RuntimeErrorKind::TypeError,
                format!("super: the parent class has no member '{name}'"),
                None,
            )),
        }
    }
}
