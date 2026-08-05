
#[path = "host/buffer/buffer.rs"]
pub mod buffer;
#[path = "host/crypto/crypto.rs"]
pub mod crypto;
#[path = "host/ffi/ffi.rs"]
pub mod ffi;
#[path = "host/fs/fs.rs"]
pub mod fs;
#[path = "globals/globals.rs"]
pub mod globals;
#[path = "host/io/io.rs"]
pub mod io;
#[path = "host/json/json.rs"]
pub mod json;
#[path = "host/math/math.rs"]
pub mod math;
#[path = "host/net/net.rs"]
pub mod net;
#[path = "primitives/mod.rs"]
pub mod primitives;
#[path = "host/reflect/reflect.rs"]
pub mod reflect;
#[path = "host/sys/sys.rs"]
pub mod sys;
#[path = "host/task/task.rs"]
pub mod task;
#[path = "host/testing/testing.rs"]
pub mod testing;
#[path = "host/time/time.rs"]
pub mod time;

use varn_types::{NativeCtx, VmValue};

pub fn build_module(id: &str, ctx: &mut dyn NativeCtx) -> Option<VmValue> {
    crate::dispatch::build_module(id, ctx)
}

pub fn has_native_builder(id: &str) -> bool {
    crate::dispatch::has_native_module_id(id)
}

pub fn force_link_builtins() -> usize {
    let dummy = std::env::var("VARN_DUMMY_LINK").is_ok() as usize;
    let mut sum = 0;

    macro_rules! register_marker {
        ($m:ident, $marker:ident) => {
            crate::dispatch::register_fallback_module_entries($m::$marker);
            sum += std::hint::black_box($m::$marker).as_ptr() as usize;
        };
    }

    register_marker!(buffer, __VARN_LINK_MARKER_RUNTIME_BUFFER);
    register_marker!(crypto, __VARN_LINK_MARKER_RUNTIME_CRYPTO);
    register_marker!(ffi, __VARN_LINK_MARKER_RUNTIME_FFI);
    register_marker!(fs, __VARN_LINK_MARKER_RUNTIME_FS);
    register_marker!(globals, __VARN_LINK_MARKER_GLOBALS);
    register_marker!(io, __VARN_LINK_MARKER_RUNTIME_IO);
    register_marker!(json, __VARN_LINK_MARKER_RUNTIME_JSON);
    register_marker!(math, __VARN_LINK_MARKER_RUNTIME_MATH);
    register_marker!(net, __VARN_LINK_MARKER_RUNTIME_NET);
    register_marker!(reflect, __VARN_LINK_MARKER_RUNTIME_REFLECT);
    register_marker!(sys, __VARN_LINK_MARKER_RUNTIME_SYS);
    register_marker!(task, __VARN_LINK_MARKER_RUNTIME_TASK);
    register_marker!(time, __VARN_LINK_MARKER_RUNTIME_TIME);

    sum + dummy
}

