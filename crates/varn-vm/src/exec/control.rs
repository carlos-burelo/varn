use crate::error::{RuntimeError, VmResult};
use crate::value::VmValue;

#[inline(always)]
pub fn jump(ip: &mut usize, offset: u16) {
    *ip += offset as usize;
}

#[inline(always)]
pub fn loop_back(ip: &mut usize, distance: u16) -> VmResult<()> {
    let dist = distance as usize;
    if dist > *ip {
        return Err(RuntimeError::new("loop: negative ip"));
    }
    *ip -= dist;
    Ok(())
}

#[inline(always)]
pub fn jump_if_false(ip: &mut usize, offset: u16, cond: VmValue) {
    if !cond.is_truthy() {
        *ip += offset as usize;
    }
}

#[inline(always)]
pub fn jump_if_true(ip: &mut usize, offset: u16, cond: VmValue) {
    if cond.is_truthy() {
        *ip += offset as usize;
    }
}
