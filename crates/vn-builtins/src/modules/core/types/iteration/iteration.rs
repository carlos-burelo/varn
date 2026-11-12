use vn_macros::vn_contract;
use vn_types::{NativeCtx, NativeError, VmValue};

pub struct GeneratorImpl;
pub struct AsyncGeneratorImpl;

fn resume(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<VmValue, NativeError> {
    ctx.resume_generator(this, VmValue::null())
}

vn_contract! {
    module: "globals",
    contract: "src/modules/core/types/iteration/iteration.vn",
    class: "Generator",
    impl GeneratorImpl {
        fn next(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<Option<VmValue>, NativeError> {
            Ok(Some(resume(ctx, this)?).filter(|value| !value.is_null()))
        }
        fn close(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<(), NativeError> {
            ctx.close_generator(this)
        }
    }
    class: "AsyncGenerator",
    impl AsyncGeneratorImpl {
        fn next(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match resume(ctx, this) {
                Ok(task) => task,
                Err(error) => {
                    let thrown = match error.thrown {
                        Some(value) => value,
                        None => ctx.alloc_str_owned(error.message),
                    };
                    ctx.task_rejected(thrown)
                }
            }
        }
        fn close(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            ctx.close_async_generator(this)
        }
    }
}
