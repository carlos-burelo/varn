use varn_macros::varn_contract;
use varn_types::{NativeCtx, NativeError, VmValue};

pub struct GeneratorImpl;
pub struct AsyncGeneratorImpl;

fn resume(ctx: &mut dyn NativeCtx, this: VmValue, input: Option<VmValue>) -> Result<VmValue, NativeError> {
    ctx.resume_generator(this, input.unwrap_or(VmValue::null()))
}

fn close(ctx: &mut dyn NativeCtx, this: VmValue, value: Option<VmValue>) -> VmValue {
    ctx.close_generator(this);
    ctx.alloc_object_with_shape(
        &varn_types::iter_result_shape(),
        vec![value.unwrap_or(VmValue::null()), VmValue::from_bool(true)],
    )
}

varn_contract! {
    module: "globals",
    contract: "src/modules/core/types/iteration/iteration.vn",
    class: "Generator",
    impl GeneratorImpl {
        fn next(ctx: &mut dyn NativeCtx, this: VmValue, value: Option<VmValue>) -> Result<VmValue, NativeError> {
            resume(ctx, this, value)
        }
        fn r#return(ctx: &mut dyn NativeCtx, this: VmValue, value: Option<VmValue>) -> VmValue {
            close(ctx, this, value)
        }
    }
    class: "AsyncGenerator",
    impl AsyncGeneratorImpl {
        fn next(ctx: &mut dyn NativeCtx, this: VmValue, value: Option<VmValue>) -> Result<VmValue, NativeError> {
            resume(ctx, this, value)
        }
        fn r#return(ctx: &mut dyn NativeCtx, this: VmValue, value: Option<VmValue>) -> VmValue {
            close(ctx, this, value)
        }
    }
}
