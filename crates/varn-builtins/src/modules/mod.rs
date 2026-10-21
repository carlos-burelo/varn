use varn_types::{NativeCtx, NativeOpEntry, VmValue};

macro_rules! native_modules {
    ($($module:ident = $path:literal => [$($entries:path),* $(,)?];)*) => {
        $(
            #[path = $path]
            pub mod $module;
        )*

        pub static NATIVE_MODULES: &[&[&NativeOpEntry]] = &[$($($entries,)*)*];
    };
}

native_modules! {
    buffer = "runtime/buffer/buffer.rs" => [buffer::NATIVE_ENTRIES];
    compress = "runtime/compress/compress.rs" => [compress::NATIVE_ENTRIES];
    crypto = "runtime/crypto/crypto.rs" => [crypto::NATIVE_ENTRIES];
    csv = "runtime/csv/csv.rs" => [csv::NATIVE_ENTRIES];
    errors = "core/errors/errors.rs" => [errors::NATIVE_ENTRIES];
    ffi = "runtime/ffi/ffi.rs" => [ffi::NATIVE_ENTRIES];
    fs = "runtime/fs/fs.rs" => [fs::NATIVE_ENTRIES, fs::FILEMODE_ENTRIES, fs::SEEKORIGIN_ENTRIES];
    globals = "core/globals/globals.rs" => [globals::NATIVE_ENTRIES];
    gpu = "runtime/gpu/gpu.rs" => [gpu::NATIVE_ENTRIES];
    io = "runtime/io/io.rs" => [io::NATIVE_ENTRIES];
    json = "runtime/json/json.rs" => [json::NATIVE_ENTRIES];
    math = "runtime/math/math.rs" => [math::NATIVE_ENTRIES];
    net = "runtime/net/net.rs" => [net::NATIVE_ENTRIES, net::http_parser::HTTPMETHOD_ENTRIES];
    path = "runtime/path/path.rs" => [path::NATIVE_ENTRIES];
    process = "runtime/process/process.rs" => [process::NATIVE_ENTRIES];
    reflect = "runtime/reflect/reflect.rs" => [reflect::NATIVE_ENTRIES];
    regex = "runtime/regex/regex.rs" => [regex::NATIVE_ENTRIES];
    sqlite = "runtime/sqlite/sqlite.rs" => [sqlite::NATIVE_ENTRIES];
    sys = "runtime/sys/sys.rs" => [sys::NATIVE_ENTRIES];
    task = "runtime/task/task.rs" => [task::NATIVE_ENTRIES];
    testing = "runtime/testing/testing.rs" => [testing::NATIVE_ENTRIES];
    text = "runtime/text/text.rs" => [text::NATIVE_ENTRIES];
    time = "runtime/time/time.rs" => [time::NATIVE_ENTRIES];
    types = "core/types/mod.rs" => [
        types::array::NATIVE_ENTRIES,
        types::bigint::NATIVE_ENTRIES,
        types::bool::NATIVE_ENTRIES,
        types::bytes::NATIVE_ENTRIES,
        types::char::NATIVE_ENTRIES,
        types::decimal::NATIVE_ENTRIES,
        types::float::NATIVE_ENTRIES,
        types::int::NATIVE_ENTRIES,
        types::iteration::NATIVE_ENTRIES,
        types::map::NATIVE_ENTRIES,
        types::range::NATIVE_ENTRIES,
        types::set::NATIVE_ENTRIES,
        types::string::NATIVE_ENTRIES,
    ];
    ui = "runtime/ui/ui.rs" => [ui::NATIVE_ENTRIES, ui::UIEVENTKIND_ENTRIES, ui::UIINJECTKIND_ENTRIES];
    ws = "runtime/ws/ws.rs" => [ws::NATIVE_ENTRIES, ws::WSOPCODE_ENTRIES];
}

pub fn build_module(id: &str, ctx: &mut dyn NativeCtx) -> Option<VmValue> {
    crate::dispatch::build_module(id, ctx)
}

pub fn has_native_builder(id: &str) -> bool {
    crate::dispatch::has_native_module_id(id)
}
