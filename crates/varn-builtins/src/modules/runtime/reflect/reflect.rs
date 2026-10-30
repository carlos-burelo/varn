use std::sync::atomic::{AtomicU64, Ordering};
use varn_macros::varn_contract;
use varn_types::NativeError;
use varn_types::{NativeCtx, VmValue};

pub struct ReflectRuntime;

static META_KEY_COUNTER: AtomicU64 = AtomicU64::new(1);

varn_contract! {
    module: "runtime:reflect",
    contract: "src/modules/runtime/reflect/reflect_runtime.vn",
    impl ReflectRuntime {
        fn defineMetadata(ctx: &mut dyn NativeCtx, metadataKey: VmValue, metadataValue: VmValue, target: VmValue) -> Result<(), NativeError> {
            let key = ctx.str_repr(metadataKey);
            ctx.define_metadata(target, &key, metadataValue);
            Ok(())
        }
        fn getMetadata(ctx: &mut dyn NativeCtx, metadataKey: VmValue, target: VmValue) -> Result<VmValue, NativeError> {
            let key = ctx.str_repr(metadataKey);
            Ok(ctx.get_metadata(target, &key).unwrap_or(VmValue::null()))
        }
        fn hasMetadata(ctx: &mut dyn NativeCtx, metadataKey: VmValue, target: VmValue) -> Result<bool, NativeError> {
            let key = ctx.str_repr(metadataKey);
            Ok(ctx.has_metadata(target, &key))
        }
        fn createMetaKey(_ctx: &mut dyn NativeCtx) -> Result<String, NativeError> {
            let id = META_KEY_COUNTER.fetch_add(1, Ordering::Relaxed);
            Ok(format!("meta_{id}"))
        }
        fn setMetaKey(ctx: &mut dyn NativeCtx, metaId: &str, target: VmValue, value: VmValue) -> Result<(), NativeError> {
            ctx.define_metadata(target, metaId, value);
            Ok(())
        }
        fn getMetaKey(ctx: &mut dyn NativeCtx, metaId: &str, target: VmValue) -> Result<VmValue, NativeError> {
            Ok(ctx.get_metadata(target, metaId).unwrap_or(VmValue::null()))
        }
        fn hasMetaKey(ctx: &mut dyn NativeCtx, metaId: &str, target: VmValue) -> Result<bool, NativeError> {
            Ok(ctx.has_metadata(target, metaId))
        }
    }
}
