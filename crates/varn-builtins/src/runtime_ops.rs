use varn_types::NativeFn;

pub type RuntimeOpFn = NativeFn;

#[derive(Clone, Copy)]
pub struct RuntimeOp {
    pub name: &'static str,
    pub func: RuntimeOpFn,
}
