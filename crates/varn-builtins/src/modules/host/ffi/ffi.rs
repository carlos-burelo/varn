use std::alloc::{alloc_zeroed, dealloc, Layout};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue, VnArray};

pub struct FfiRuntime;

#[derive(Debug)]
struct LoadedLib(libloading::Library);

const ERR_PERMISSION_DENIED: &str = "E_HOST_PERMISSION_DENIED:id=runtime:ffi:capability=sys.ffi";
const HEADER_SIZE: usize = 16;

varn_contract! {
    module: "runtime:ffi",
    contract: "src/modules/host/ffi/ffi_runtime.vn",
    impl FfiRuntime {
        fn dlopen(ctx: &mut dyn NativeCtx, path: &str) -> Result<i64, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err(ERR_PERMISSION_DENIED.to_string());
            }
            unsafe {
                let lib = libloading::Library::new(path)
                    .map_err(|e| format!("ffi.dlopen: failed to load '{path}': {e}"))?;
                let resource_id = ctx.resources().insert(LoadedLib(lib));
                Ok(resource_id as i64)
            }
        }

        fn dlclose(ctx: &mut dyn NativeCtx, libHandle: i64) -> Result<bool, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err(ERR_PERMISSION_DENIED.to_string());
            }
            let handle_id = libHandle as u32;
            let closed = ctx.resources().remove::<LoadedLib>(handle_id).is_some();
            Ok(closed)
        }

        fn dlsym(ctx: &mut dyn NativeCtx, libHandle: i64, symbol: &str) -> Result<i64, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err(ERR_PERMISSION_DENIED.to_string());
            }
            let handle_id = libHandle as u32;
            let res = ctx.resources().get::<LoadedLib>(handle_id)
                .ok_or_else(|| "ffi.dlsym: invalid library handle".to_string())?;
            unsafe {
                let c_str = std::ffi::CString::new(symbol)
                    .map_err(|e| format!("ffi.dlsym: invalid symbol name: {e}"))?;
                let sym: libloading::Symbol<*const ()> = res.0.get(c_str.as_bytes_with_nul())
                    .map_err(|e| format!("ffi.dlsym: symbol '{symbol}' not found: {e}"))?;
                let ptr = *sym as usize as i64;
                Ok(ptr)
            }
        }

        fn call(ctx: &mut dyn NativeCtx, fnPtr: i64, retType: i64, args: VnArray) -> Result<VmValue, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err(ERR_PERMISSION_DENIED.to_string());
            }
            let ptr_val = fnPtr as usize;
            if ptr_val == 0 {
                return Err("ffi.call: null function pointer".to_string());
            }

            let argc = args.len(ctx);
            if argc > 8 {
                return Err("ffi.call: maximum 8 arguments supported".to_string());
            }

            let mut raw_args = [0usize; 8];
            for (i, slot) in raw_args.iter_mut().enumerate().take(argc) {
                let v = args.get(ctx, i).unwrap_or_else(VmValue::null);
                if v.is_int() {
                    *slot = v.as_int() as usize;
                } else if v.is_f64() {
                    *slot = v.as_f64().to_bits() as usize;
                } else if v.is_bool() {
                    *slot = if v.as_bool() { 1 } else { 0 };
                } else if v.is_null() {
                    *slot = 0;
                } else {
                    *slot = ctx.as_int(v) as usize;
                }
            }

            unsafe {
                match retType {
                    0 => {
                        match argc {
                            0 => { let f: extern "C" fn() = std::mem::transmute(ptr_val); f(); }
                            1 => { let f: extern "C" fn(usize) = std::mem::transmute(ptr_val); f(raw_args[0]); }
                            2 => { let f: extern "C" fn(usize, usize) = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1]); }
                            3 => { let f: extern "C" fn(usize, usize, usize) = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2]); }
                            4 => { let f: extern "C" fn(usize, usize, usize, usize) = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3]); }
                            5 => { let f: extern "C" fn(usize, usize, usize, usize, usize) = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3], raw_args[4]); }
                            6 => { let f: extern "C" fn(usize, usize, usize, usize, usize, usize) = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3], raw_args[4], raw_args[5]); }
                            7 => { let f: extern "C" fn(usize, usize, usize, usize, usize, usize, usize) = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3], raw_args[4], raw_args[5], raw_args[6]); }
                            8 => { let f: extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize) = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3], raw_args[4], raw_args[5], raw_args[6], raw_args[7]); }
                            _ => unreachable!(),
                        }
                        Ok(VmValue::null())
                    }
                    2 => {
                        let res = match argc {
                            0 => { let f: extern "C" fn() -> f64 = std::mem::transmute(ptr_val); f() }
                            1 => { let f: extern "C" fn(usize) -> f64 = std::mem::transmute(ptr_val); f(raw_args[0]) }
                            2 => { let f: extern "C" fn(usize, usize) -> f64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1]) }
                            3 => { let f: extern "C" fn(usize, usize, usize) -> f64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2]) }
                            4 => { let f: extern "C" fn(usize, usize, usize, usize) -> f64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3]) }
                            5 => { let f: extern "C" fn(usize, usize, usize, usize, usize) -> f64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3], raw_args[4]) }
                            6 => { let f: extern "C" fn(usize, usize, usize, usize, usize, usize) -> f64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3], raw_args[4], raw_args[5]) }
                            7 => { let f: extern "C" fn(usize, usize, usize, usize, usize, usize, usize) -> f64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3], raw_args[4], raw_args[5], raw_args[6]) }
                            8 => { let f: extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize) -> f64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3], raw_args[4], raw_args[5], raw_args[6], raw_args[7]) }
                            _ => unreachable!(),
                        };
                        Ok(ctx.intern(varn_types::Value::Float(res)))
                    }
                    _ => {
                        let res: i64 = match argc {
                            0 => { let f: extern "C" fn() -> i64 = std::mem::transmute(ptr_val); f() }
                            1 => { let f: extern "C" fn(usize) -> i64 = std::mem::transmute(ptr_val); f(raw_args[0]) }
                            2 => { let f: extern "C" fn(usize, usize) -> i64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1]) }
                            3 => { let f: extern "C" fn(usize, usize, usize) -> i64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2]) }
                            4 => { let f: extern "C" fn(usize, usize, usize, usize) -> i64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3]) }
                            5 => { let f: extern "C" fn(usize, usize, usize, usize, usize) -> i64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3], raw_args[4]) }
                            6 => { let f: extern "C" fn(usize, usize, usize, usize, usize, usize) -> i64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3], raw_args[4], raw_args[5]) }
                            7 => { let f: extern "C" fn(usize, usize, usize, usize, usize, usize, usize) -> i64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3], raw_args[4], raw_args[5], raw_args[6]) }
                            8 => { let f: extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize) -> i64 = std::mem::transmute(ptr_val); f(raw_args[0], raw_args[1], raw_args[2], raw_args[3], raw_args[4], raw_args[5], raw_args[6], raw_args[7]) }
                            _ => unreachable!(),
                        };
                        if retType == 3 {
                            if res == 0 {
                                Ok(VmValue::null())
                            } else {
                                let c_str = std::ffi::CStr::from_ptr(res as *const std::ffi::c_char);
                                let s = c_str.to_string_lossy();
                                Ok(ctx.alloc_str(&s))
                            }
                        } else if retType == 4 {
                            Ok(ctx.bool_val(res != 0))
                        } else {
                            Ok(ctx.int_val(res))
                        }
                    }
                }
            }
        }

        fn alloc(ctx: &mut dyn NativeCtx, size: i64) -> Result<i64, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err(ERR_PERMISSION_DENIED.to_string());
            }
            if size <= 0 {
                return Err("ffi.alloc: size must be greater than 0".to_string());
            }
            let total_size = (size as usize) + HEADER_SIZE;
            let layout = Layout::from_size_align(total_size, 16)
                .map_err(|e| format!("ffi.alloc: invalid layout: {e}"))?;
            unsafe {
                let raw = alloc_zeroed(layout);
                if raw.is_null() {
                    return Err("ffi.alloc: out of memory".to_string());
                }
                *(raw as *mut u64) = size as u64;
                let user_ptr = raw.add(HEADER_SIZE);
                Ok(user_ptr as usize as i64)
            }
        }

        fn free(ctx: &mut dyn NativeCtx, ptr: i64) -> Result<(), String> {
            if !ctx.has_capability("sys.ffi") {
                return Err(ERR_PERMISSION_DENIED.to_string());
            }
            if ptr == 0 {
                return Ok(());
            }
            unsafe {
                let raw = (ptr as *mut u8).sub(HEADER_SIZE);
                let size = *(raw as *const u64) as usize;
                let total_size = size + HEADER_SIZE;
                if let Ok(layout) = Layout::from_size_align(total_size, 16) {
                    dealloc(raw, layout);
                }
            }
            Ok(())
        }

        fn readInt(ctx: &mut dyn NativeCtx, ptr: i64, offset: i64, size: i64) -> Result<i64, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err(ERR_PERMISSION_DENIED.to_string());
            }
            if ptr == 0 {
                return Err("ffi.readInt: null pointer dereference".to_string());
            }
            unsafe {
                let addr = (ptr + offset) as *const u8;
                match size {
                    1 => Ok(*addr as i8 as i64),
                    2 => Ok(*(addr as *const i16) as i64),
                    4 => Ok(*(addr as *const i32) as i64),
                    8 => Ok(*(addr as *const i64)),
                    _ => Err(format!("ffi.readInt: unsupported size {size}, expected 1, 2, 4 or 8")),
                }
            }
        }

        fn writeInt(ctx: &mut dyn NativeCtx, ptr: i64, offset: i64, val: i64, size: i64) -> Result<(), String> {
            if !ctx.has_capability("sys.ffi") {
                return Err(ERR_PERMISSION_DENIED.to_string());
            }
            if ptr == 0 {
                return Err("ffi.writeInt: null pointer dereference".to_string());
            }
            unsafe {
                let addr = (ptr + offset) as *mut u8;
                match size {
                    1 => *addr = val as u8,
                    2 => *(addr as *mut i16) = val as i16,
                    4 => *(addr as *mut i32) = val as i32,
                    8 => *(addr as *mut i64) = val,
                    _ => return Err(format!("ffi.writeInt: unsupported size {size}, expected 1, 2, 4 or 8")),
                }
            }
            Ok(())
        }

        fn readFloat(ctx: &mut dyn NativeCtx, ptr: i64, offset: i64, isDouble: bool) -> Result<f64, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err(ERR_PERMISSION_DENIED.to_string());
            }
            if ptr == 0 {
                return Err("ffi.readFloat: null pointer dereference".to_string());
            }
            unsafe {
                let addr = (ptr + offset) as *const u8;
                if isDouble {
                    Ok(*(addr as *const f64))
                } else {
                    Ok(*(addr as *const f32) as f64)
                }
            }
        }

        fn writeFloat(ctx: &mut dyn NativeCtx, ptr: i64, offset: i64, val: f64, isDouble: bool) -> Result<(), String> {
            if !ctx.has_capability("sys.ffi") {
                return Err(ERR_PERMISSION_DENIED.to_string());
            }
            if ptr == 0 {
                return Err("ffi.writeFloat: null pointer dereference".to_string());
            }
            unsafe {
                let addr = (ptr + offset) as *mut u8;
                if isDouble {
                    *(addr as *mut f64) = val;
                } else {
                    *(addr as *mut f32) = val as f32;
                }
            }
            Ok(())
        }

        fn allocCString(ctx: &mut dyn NativeCtx, s: &str) -> Result<i64, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err(ERR_PERMISSION_DENIED.to_string());
            }
            let bytes = s.as_bytes();
            let size = bytes.len() + 1;
            let total_size = size + HEADER_SIZE;
            let layout = Layout::from_size_align(total_size, 16)
                .map_err(|e| format!("ffi.allocCString: invalid layout: {e}"))?;
            unsafe {
                let raw = alloc_zeroed(layout);
                if raw.is_null() {
                    return Err("ffi.allocCString: out of memory".to_string());
                }
                *(raw as *mut u64) = size as u64;
                let user_ptr = raw.add(HEADER_SIZE);
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), user_ptr, bytes.len());
                *user_ptr.add(bytes.len()) = 0;
                Ok(user_ptr as usize as i64)
            }
        }

        fn readCString(ctx: &mut dyn NativeCtx, ptr: i64, offset: i64) -> Result<String, String> {
            if !ctx.has_capability("sys.ffi") {
                return Err(ERR_PERMISSION_DENIED.to_string());
            }
            if ptr == 0 {
                return Err("ffi.readCString: null pointer dereference".to_string());
            }
            unsafe {
                let addr = (ptr + offset) as *const u8;
                let mut len = 0usize;
                while len < 65536 && *addr.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(addr, len);
                Ok(String::from_utf8_lossy(slice).into_owned())
            }
        }
    }
}
