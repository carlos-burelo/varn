use varn_core::{RuntimeErrorKind, StdErrorClass};
use varn_macros::varn_contract;
use varn_types::{NativeCtx, NativeError, VmValue, VnArray};

pub struct StdErrorRuntime;

fn range_error(message: String) -> NativeError {
    NativeError::rethrow(RuntimeErrorKind::RangeError, message, None)
}

varn_contract! {
    module: "runtime:error",
    contract: "src/modules/runtime/error/error_runtime.vn",
    impl StdErrorRuntime {
        fn errorCode(_ctx: &mut dyn NativeCtx, className: &str) -> Result<String, NativeError> {
            StdErrorClass::of_name(className)
                .map(|class| class.code().to_owned())
                .ok_or_else(|| range_error(format!("'{className}' is not a std error class")))
        }

        fn registerErrorClasses(ctx: &mut dyn NativeCtx, classes: VnArray) -> Result<(), NativeError> {
            let mut seen = Vec::new();
            for index in 0..classes.len(ctx) {
                let value: VmValue = classes.get(ctx, index).unwrap_or_else(VmValue::null);
                let cls = ctx
                    .class_from_value(value)
                    .ok_or_else(|| range_error(format!("registerErrorClasses: item {index} is not a class")))?;
                let class = StdErrorClass::of_name(&cls.name).ok_or_else(|| {
                    range_error(format!("'{}' is not in the std error table", cls.name))
                })?;
                let parent = cls.superclass.borrow().as_ref().map(|p| p.name.clone());
                let expected = class
                    .parent()
                    .map_or(RuntimeErrorKind::Error.class_name(), StdErrorClass::name);
                if parent.as_deref() != Some(expected) {
                    return Err(range_error(format!(
                        "'{}' must extend '{expected}', it extends '{}'",
                        cls.name,
                        parent.as_deref().unwrap_or("nothing")
                    )));
                }
                ctx.register_class(class.name(), cls);
                seen.push(class);
            }
            for class in StdErrorClass::ALL {
                if !seen.contains(&class) {
                    return Err(range_error(format!(
                        "registerErrorClasses: '{}' is missing",
                        class.name()
                    )));
                }
            }
            Ok(())
        }
    }
}
