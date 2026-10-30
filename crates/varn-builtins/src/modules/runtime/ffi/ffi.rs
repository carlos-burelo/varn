use varn_macros::varn_contract;
use varn_types::{NativeCtx, VmValue, VnArray};

#[path = "call.rs"]
mod call;
#[path = "memory.rs"]
mod memory;

pub struct FfiRuntime;

#[derive(Debug)]
struct LoadedLib {
    lib: libloading::Library,
    is_system_lib: bool,
}

const ERR_PERMISSION_DENIED: &str = "E_HOST_PERMISSION_DENIED:id=runtime:ffi:capability=sys.ffi";

fn permit(ctx: &mut dyn NativeCtx) -> Result<(), String> {
    if ctx.has_capability("sys.ffi") {
        Ok(())
    } else {
        Err(ERR_PERMISSION_DENIED.to_string())
    }
}

fn scoped<T>(who: &str, result: Result<T, String>) -> Result<T, String> {
    result.map_err(|e| format!("ffi.{who}: {e}"))
}

varn_contract! {
    module: "runtime:ffi",
    contract: "src/modules/runtime/ffi/ffi_runtime.vn",
    impl FfiRuntime {
        fn dlopen(ctx: &mut dyn NativeCtx, path: &str) -> Result<i64, String> {
            permit(ctx)?;
            let is_system_lib = path == "libc.so.6"
                || path == "libc.so"
                || path.starts_with("libc.")
                || path == "libSystem.B.dylib"
                || path.eq_ignore_ascii_case("kernel32.dll");

            unsafe {
                let lib = libloading::Library::new(path)
                    .map_err(|e| format!("ffi.dlopen: failed to load '{path}': {e}"))?;
                let resource_id = ctx.resources().insert(LoadedLib { lib, is_system_lib });
                Ok(resource_id as i64)
            }
        }

        fn dlclose(ctx: &mut dyn NativeCtx, libHandle: i64) -> Result<bool, String> {
            permit(ctx)?;
            let handle_id = libHandle as u32;
            if let Some(loaded) = ctx.resources().remove::<LoadedLib>(handle_id) {
                if loaded.is_system_lib {
                    std::mem::forget(loaded.lib);
                }
                Ok(true)
            } else {
                Ok(false)
            }
        }

        fn dlsym(ctx: &mut dyn NativeCtx, libHandle: i64, symbol: &str) -> Result<i64, String> {
            permit(ctx)?;
            let handle_id = libHandle as u32;
            let res = ctx.resources().get::<LoadedLib>(handle_id)
                .ok_or_else(|| "ffi.dlsym: invalid library handle".to_string())?;

            unsafe {
                let c_str = std::ffi::CString::new(symbol)
                    .map_err(|e| format!("ffi.dlsym: invalid symbol name: {e}"))?;
                let sym: libloading::Symbol<*const ()> = res.lib.get(c_str.as_bytes_with_nul())
                    .map_err(|e| format!("ffi.dlsym: symbol '{symbol}' not found: {e}"))?;
                let ptr = *sym as usize as i64;
                Ok(ptr)
            }
        }

        fn call(ctx: &mut dyn NativeCtx, fnPtr: i64, retType: i64, paramTypes: VnArray, args: VnArray) -> Result<VmValue, String> {
            permit(ctx)?;
            call::call(ctx, fnPtr, retType, paramTypes, args)
        }

        fn alloc(ctx: &mut dyn NativeCtx, size: i64) -> Result<i64, String> {
            permit(ctx)?;
            scoped("alloc", memory::allocate(size)).map(|addr| addr as i64)
        }

        fn free(ctx: &mut dyn NativeCtx, ptr: i64) -> Result<(), String> {
            permit(ctx)?;
            scoped("free", memory::release(ptr as usize))
        }

        fn readInt(ctx: &mut dyn NativeCtx, ptr: i64, offset: i64, size: i64) -> Result<i64, String> {
            permit(ctx)?;
            let width = match size {
                1 | 2 | 4 | 8 => size as usize,
                _ => return Err(format!("ffi.readInt: unsupported size {size}, expected 1, 2, 4 or 8")),
            };
            let addr = scoped("readInt", memory::span(ptr, offset, width))?;
            unsafe {
                Ok(match width {
                    1 => std::ptr::read_unaligned(addr as *const i8) as i64,
                    2 => std::ptr::read_unaligned(addr as *const i16) as i64,
                    4 => std::ptr::read_unaligned(addr as *const i32) as i64,
                    _ => std::ptr::read_unaligned(addr as *const i64),
                })
            }
        }

        fn writeInt(ctx: &mut dyn NativeCtx, ptr: i64, offset: i64, val: i64, size: i64) -> Result<(), String> {
            permit(ctx)?;
            let width = match size {
                1 | 2 | 4 | 8 => size as usize,
                _ => return Err(format!("ffi.writeInt: unsupported size {size}, expected 1, 2, 4 or 8")),
            };
            let addr = scoped("writeInt", memory::span(ptr, offset, width))?;
            unsafe {
                match width {
                    1 => std::ptr::write_unaligned(addr as *mut u8, val as u8),
                    2 => std::ptr::write_unaligned(addr as *mut i16, val as i16),
                    4 => std::ptr::write_unaligned(addr as *mut i32, val as i32),
                    _ => std::ptr::write_unaligned(addr as *mut i64, val),
                }
            }
            Ok(())
        }

        fn readFloat(ctx: &mut dyn NativeCtx, ptr: i64, offset: i64, isDouble: bool) -> Result<f64, String> {
            permit(ctx)?;
            let width = if isDouble { 8 } else { 4 };
            let addr = scoped("readFloat", memory::span(ptr, offset, width))?;
            unsafe {
                if isDouble {
                    Ok(std::ptr::read_unaligned(addr as *const f64))
                } else {
                    Ok(std::ptr::read_unaligned(addr as *const f32) as f64)
                }
            }
        }

        fn writeFloat(ctx: &mut dyn NativeCtx, ptr: i64, offset: i64, val: f64, isDouble: bool) -> Result<(), String> {
            permit(ctx)?;
            let width = if isDouble { 8 } else { 4 };
            let addr = scoped("writeFloat", memory::span(ptr, offset, width))?;
            unsafe {
                if isDouble {
                    std::ptr::write_unaligned(addr as *mut f64, val);
                } else {
                    std::ptr::write_unaligned(addr as *mut f32, val as f32);
                }
            }
            Ok(())
        }

        fn allocCString(ctx: &mut dyn NativeCtx, s: &str) -> Result<i64, String> {
            permit(ctx)?;
            scoped("allocCString", memory::allocate_cstring(s)).map(|addr| addr as i64)
        }

        fn readCString(ctx: &mut dyn NativeCtx, ptr: i64, offset: i64) -> Result<String, String> {
            permit(ctx)?;
            let (start, limit) = scoped("readCString", memory::cstring_bounds(ptr, offset))?;
            unsafe {
                let addr = start as *const u8;
                let mut len = 0usize;
                while len < limit && *addr.add(len) != 0 {
                    len += 1;
                }
                if len == limit {
                    return Err("ffi.readCString: string is not NUL-terminated".to_string());
                }
                let slice = std::slice::from_raw_parts(addr, len);
                Ok(String::from_utf8_lossy(slice).into_owned())
            }
        }
    }
}
