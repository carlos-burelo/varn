use crate::{NativeError, NativeFnResult, VmValue};

pub trait Boundary {
    fn conform(&self, value: VmValue, desc: &str, witnesses: &[VmValue]) -> NativeFnResult;
    fn verify_call(&self, callee: VmValue, args: &[VmValue]) -> Result<(), NativeError>;
    fn method_callee(&self, recv: VmValue, name: &str) -> VmValue;
    fn class_method(&self, class: VmValue, name: &str) -> VmValue;
}
