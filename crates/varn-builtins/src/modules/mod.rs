#[path = "std/collections/collections.rs"]
pub mod collections;
#[path = "std/core/core.rs"]
pub mod core;
#[path = "std/crypto/crypto.rs"]
pub mod crypto;
#[path = "globals/errors.rs"]
pub mod errors;
#[path = "std/fs/fs.rs"]
pub mod fs;
#[path = "globals/globals.rs"]
pub mod globals;
#[path = "std/http/http.rs"]
pub mod http;
#[path = "std/http/http_helpers.rs"]
mod http_helpers;
#[path = "std/io/io.rs"]
pub mod io;
#[path = "std/json/json.rs"]
pub mod json;
#[path = "std/math/math.rs"]
pub mod math;
#[path = "std/net/net.rs"]
pub mod net;
#[path = "std/option/option.rs"]
pub mod option;
#[path = "std/path/path.rs"]
pub mod path;
#[path = "primitives/mod.rs"]
pub mod primitives;
#[path = "globals/print.rs"]
pub mod print;
#[path = "std/reflect/reflect.rs"]
pub mod reflect;
#[path = "globals/symbols.rs"]
pub mod symbols;
#[path = "std/sys/sys.rs"]
pub mod sys;
#[path = "std/task/task.rs"]
pub mod task;
#[path = "std/testing/testing.rs"]
pub mod testing;
#[path = "std/time/time.rs"]
pub mod time;

use varn_types::{NativeCtx, VmValue};

pub fn build_module(id: &str, ctx: &mut dyn NativeCtx) -> Option<VmValue> {
    crate::dispatch::build_module(id, ctx)
}

pub fn has_native_builder(id: &str) -> bool {
    crate::dispatch::has_native_module_id(id)
}
