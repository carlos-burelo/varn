use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct FfiRuntime;

#[derive(Debug)]
struct LoadedLib(libloading::Library);

varn_contract! {
    module: "runtime:ffi",
    contract: "src/modules/host/ffi/ffi_runtime.vn",
    impl FfiRuntime {
        fn dlopen(ctx: &mut dyn NativeCtx, path: &str) -> Result<VmValue, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err("E_HOST_PERMISSION_DENIED:id=runtime:ffi:capability=sys.ffi".to_string());
            }
            unsafe {
                let lib = libloading::Library::new(path)
                    .map_err(|e| format!("ffi.dlopen: failed to load '{path}': {e}"))?;
                let resource_id = ctx.resources().insert(LoadedLib(lib));
                Ok(ctx.int_val(resource_id as i64))
            }
        }

        fn dlsym(ctx: &mut dyn NativeCtx, libHandle: VmValue, symbol: &str) -> Result<VmValue, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err("E_HOST_PERMISSION_DENIED:id=runtime:ffi:capability=sys.ffi".to_string());
            }
            let handle_id = ctx.as_int(libHandle) as u32;
            let res = ctx.resources().get::<LoadedLib>(handle_id)
                .ok_or_else(|| "ffi.dlsym: invalid library handle".to_string())?;
            unsafe {
                let c_str = std::ffi::CString::new(symbol)
                    .map_err(|e| format!("ffi.dlsym: invalid symbol name: {e}"))?;
                let sym: libloading::Symbol<*const ()> = res.0.get(c_str.as_bytes_with_nul())
                    .map_err(|e| format!("ffi.dlsym: symbol '{symbol}' not found: {e}"))?;
                let ptr = *sym as usize as i64;
                Ok(ctx.int_val(ptr))
            }
        }

        fn callI64(ctx: &mut dyn NativeCtx, fnPtr: VmValue, a1: i64, a2: i64) -> Result<i64, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err("E_HOST_PERMISSION_DENIED:id=runtime:ffi:capability=sys.ffi".to_string());
            }
            let ptr_val = ctx.as_int(fnPtr) as usize;
            if ptr_val == 0 {
                return Err("ffi.callI64: null function pointer".to_string());
            }
            unsafe {
                let func: extern "C" fn(i64, i64) -> i64 = std::mem::transmute(ptr_val);
                Ok(func(a1, a2))
            }
        }

        fn callF64(ctx: &mut dyn NativeCtx, fnPtr: VmValue, a1: f64, a2: f64) -> Result<f64, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err("E_HOST_PERMISSION_DENIED:id=runtime:ffi:capability=sys.ffi".to_string());
            }
            let ptr_val = ctx.as_int(fnPtr) as usize;
            if ptr_val == 0 {
                return Err("ffi.callF64: null function pointer".to_string());
            }
            unsafe {
                let func: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(ptr_val);
                Ok(func(a1, a2))
            }
        }
    }
}
