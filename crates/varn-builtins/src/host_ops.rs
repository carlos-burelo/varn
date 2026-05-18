use varn_types::NativeFn;

pub type HostOpFn = NativeFn;

#[derive(Clone, Copy)]
pub struct HostOp {
    pub name: &'static str,
    pub func: HostOpFn,
}
