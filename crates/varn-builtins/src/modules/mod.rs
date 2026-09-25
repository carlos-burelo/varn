#[path = "runtime/buffer/buffer.rs"]
pub mod buffer;
#[path = "runtime/compress/compress.rs"]
pub mod compress;
#[path = "runtime/crypto/crypto.rs"]
pub mod crypto;
#[path = "runtime/csv/csv.rs"]
pub mod csv;
#[path = "core/errors/errors.rs"]
pub mod errors;
#[path = "runtime/ffi/ffi.rs"]
pub mod ffi;
#[path = "runtime/fs/fs.rs"]
pub mod fs;
#[path = "core/globals/globals.rs"]
pub mod globals;
#[path = "runtime/io/io.rs"]
pub mod io;
#[path = "runtime/json/json.rs"]
pub mod json;
#[path = "runtime/math/math.rs"]
pub mod math;
#[path = "runtime/net/net.rs"]
pub mod net;

#[path = "runtime/process/process.rs"]
pub mod process;
#[path = "runtime/reflect/reflect.rs"]
pub mod reflect;
#[path = "runtime/regex/regex.rs"]
pub mod regex;
#[path = "runtime/sqlite/sqlite.rs"]
pub mod sqlite;
#[path = "runtime/sys/sys.rs"]
pub mod sys;
#[path = "runtime/task/task.rs"]
pub mod task;
#[path = "runtime/testing/testing.rs"]
pub mod testing;
#[path = "runtime/time/time.rs"]
pub mod time;
#[path = "core/types/mod.rs"]
pub mod types;
#[path = "runtime/ws/ws.rs"]
pub mod ws;

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
        ($($m:ident)::+, $marker:ident) => {
            crate::dispatch::register_fallback_module_entries($($m)::*::$marker);
            sum += std::hint::black_box($($m)::*::$marker).as_ptr() as usize;
        };
    }

    register_marker!(buffer, __VARN_LINK_MARKER_RUNTIME_BUFFER);
    register_marker!(compress, __VARN_LINK_MARKER_RUNTIME_COMPRESS);
    register_marker!(crypto, __VARN_LINK_MARKER_RUNTIME_CRYPTO);
    register_marker!(csv, __VARN_LINK_MARKER_RUNTIME_CSV);
    register_marker!(ffi, __VARN_LINK_MARKER_RUNTIME_FFI);
    register_marker!(fs, __VARN_LINK_MARKER_RUNTIME_FS);
    register_marker!(globals, __VARN_LINK_MARKER_GLOBALS);
    register_marker!(errors, __VARN_LINK_MARKER_ERROR);
    register_marker!(errors, __VARN_LINK_MARKER_TYPEERROR);
    register_marker!(errors, __VARN_LINK_MARKER_RANGEERROR);
    register_marker!(io, __VARN_LINK_MARKER_RUNTIME_IO);
    register_marker!(json, __VARN_LINK_MARKER_RUNTIME_JSON);
    register_marker!(math, __VARN_LINK_MARKER_RUNTIME_MATH);
    register_marker!(net, __VARN_LINK_MARKER_RUNTIME_NET);
    register_marker!(process, __VARN_LINK_MARKER_RUNTIME_PROCESS);
    register_marker!(reflect, __VARN_LINK_MARKER_RUNTIME_REFLECT);
    register_marker!(regex, __VARN_LINK_MARKER_RUNTIME_REGEX);
    register_marker!(sqlite, __VARN_LINK_MARKER_RUNTIME_SQLITE);
    register_marker!(sys, __VARN_LINK_MARKER_RUNTIME_SYS);
    register_marker!(task, __VARN_LINK_MARKER_RUNTIME_TASK);
    register_marker!(task, __VARN_LINK_MARKER_ISOLATEHANDLE);
    register_marker!(task, __VARN_LINK_MARKER_SENDER);
    register_marker!(task, __VARN_LINK_MARKER_RECEIVER);
    register_marker!(task, __VARN_LINK_MARKER_CHANNEL);
    register_marker!(task, __VARN_LINK_MARKER_CHANNELCLOSED);
    register_marker!(time, __VARN_LINK_MARKER_RUNTIME_TIME);
    register_marker!(ws, __VARN_LINK_MARKER_RUNTIME_WS);

    register_marker!(types::array, __VARN_LINK_MARKER_ARRAY);
    register_marker!(types::bigint, __VARN_LINK_MARKER_BIGINT);
    register_marker!(types::bytes, __VARN_LINK_MARKER_BYTES);
    register_marker!(types::bool, __VARN_LINK_MARKER_BOOL);
    register_marker!(types::char, __VARN_LINK_MARKER_CHAR);
    register_marker!(types::decimal, __VARN_LINK_MARKER_DECIMAL);
    register_marker!(types::float, __VARN_LINK_MARKER_FLOAT);
    register_marker!(types::int, __VARN_LINK_MARKER_INT);
    register_marker!(types::map, __VARN_LINK_MARKER_MAP);
    register_marker!(types::range, __VARN_LINK_MARKER_RANGE);
    register_marker!(types::set, __VARN_LINK_MARKER_SET);
    register_marker!(types::string, __VARN_LINK_MARKER_STR);

    sum + dummy
}
