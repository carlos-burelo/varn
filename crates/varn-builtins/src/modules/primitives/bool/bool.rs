use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, NativeFnResult, VmValue};

/// Native implementation backing the `bool` contract
/// (`src/modules/primitives/bool/bool.vn`).
pub struct Bool;

varn_contract! {
    module: "globals",
    class: "bool",
    contract: "src/modules/primitives/bool/bool.vn",
    impl Bool {
        fn toString(_ctx: &mut dyn NativeCtx, this: bool) -> String {
            if this { "true".to_string() } else { "false".to_string() }
        }

        fn valueOf(_ctx: &mut dyn NativeCtx, this: bool) -> bool {
            this
        }
    }
}

/// Free helper re-exported from `modules/mod.rs`; unrelated to the contract.
pub fn boolean_to_string(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let v = args.first().copied().unwrap_or(VmValue::null());
    Ok(ctx.alloc_str(if v.as_bool() { "true" } else { "false" }))
}
