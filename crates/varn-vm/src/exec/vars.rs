use crate::error::{RuntimeError, VmResult};
use crate::frame::{CallFrame, VmUpvalue};
use crate::globals::GlobalStore;
use crate::value::VmValue;

#[inline(always)]
pub fn get_local(stack: &[VmValue], base: usize, slot: usize) -> VmResult<VmValue> {
    stack
        .get(base + slot)
        .copied()
        .ok_or_else(|| RuntimeError::new(format!("local slot {} out of range", slot)))
}

#[inline(always)]
pub fn set_local(stack: &mut Vec<VmValue>, base: usize, slot: usize, val: VmValue) -> VmResult<()> {
    let idx = base + slot;
    while stack.len() <= idx {
        stack.push(VmValue::null());
    }
    stack[idx] = val;
    Ok(())
}

#[inline(always)]
pub fn get_global_by_name(name: &str, globals: &GlobalStore) -> VmValue {
    globals.get_by_name(name).unwrap_or(VmValue::null())
}

#[inline(always)]
pub fn get_global_by_index(idx: usize, globals: &GlobalStore) -> VmValue {
    globals.get_by_index(idx).unwrap_or(VmValue::null())
}

#[inline(always)]
pub fn set_global_by_name(name: &str, val: VmValue, globals: &mut GlobalStore) {
    globals.set_by_name(name, val);
}

#[inline(always)]
pub fn set_global_by_index(idx: usize, val: VmValue, globals: &mut GlobalStore) {
    globals.set_by_index(idx, val);
}

#[inline(always)]
pub fn define_global(name: &str, val: VmValue, globals: &mut GlobalStore) {
    globals.define(name, val);
}

#[inline(always)]
pub fn get_upvalue(frame: &CallFrame, upv_idx: usize, stack: &[VmValue]) -> VmResult<VmValue> {
    let uv = frame
        .closure
        .upvalues
        .get(upv_idx)
        .ok_or_else(|| RuntimeError::new(format!("upvalue {} out of range", upv_idx)))?;
    Ok(uv.read(stack))
}

#[inline(always)]
pub fn set_upvalue(
    frame: &CallFrame,
    upv_idx: usize,
    val: VmValue,
    stack: &mut Vec<VmValue>,
) -> VmResult<()> {
    let uv = frame
        .closure
        .upvalues
        .get(upv_idx)
        .ok_or_else(|| RuntimeError::new(format!("upvalue {} out of range", upv_idx)))?;
    uv.write(val, stack);
    Ok(())
}
