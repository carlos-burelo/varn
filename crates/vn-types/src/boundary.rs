use crate::{NativeError, NativeFnResult, VmValue};

pub trait Boundary {
    fn conform(&mut self, value: VmValue, desc: &str, witnesses: &[VmValue]) -> NativeFnResult;
    fn decode(&mut self, value: VmValue, desc: &str, witnesses: &[VmValue]) -> NativeFnResult;
    fn verify_call(&mut self, callee: VmValue, args: &[VmValue]) -> Result<(), NativeError>;
    fn method_callee(&self, recv: VmValue, name: &str) -> VmValue;
    fn class_method(&self, class: VmValue, name: &str) -> VmValue;
    fn inherited(
        &mut self,
        home: VmValue,
        receiver: VmValue,
        name: &str,
        accessor: bool,
    ) -> NativeFnResult;
}
