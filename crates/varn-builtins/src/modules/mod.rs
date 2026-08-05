
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
    sum += std::hint::black_box(buffer::__VARN_LINK_MARKER_RUNTIME_BUFFER).as_ptr() as usize;
    sum += std::hint::black_box(crypto::__VARN_LINK_MARKER_RUNTIME_CRYPTO).as_ptr() as usize;
    sum += std::hint::black_box(ffi::__VARN_LINK_MARKER_RUNTIME_FFI).as_ptr() as usize;
    sum += std::hint::black_box(fs::__VARN_LINK_MARKER_RUNTIME_FS).as_ptr() as usize;
    sum += std::hint::black_box(globals::__VARN_LINK_MARKER_GLOBALS).as_ptr() as usize;
    sum += std::hint::black_box(io::__VARN_LINK_MARKER_RUNTIME_IO).as_ptr() as usize;
    sum += std::hint::black_box(json::__VARN_LINK_MARKER_RUNTIME_JSON).as_ptr() as usize;
    sum += std::hint::black_box(math::__VARN_LINK_MARKER_RUNTIME_MATH).as_ptr() as usize;
    sum += std::hint::black_box(net::__VARN_LINK_MARKER_RUNTIME_NET).as_ptr() as usize;
    sum += std::hint::black_box(reflect::__VARN_LINK_MARKER_RUNTIME_REFLECT).as_ptr() as usize;
    sum += std::hint::black_box(sys::__VARN_LINK_MARKER_RUNTIME_SYS).as_ptr() as usize;
    sum += std::hint::black_box(task::__VARN_LINK_MARKER_RUNTIME_TASK).as_ptr() as usize;
    sum += std::hint::black_box(time::__VARN_LINK_MARKER_RUNTIME_TIME).as_ptr() as usize;
    sum + dummy
}

