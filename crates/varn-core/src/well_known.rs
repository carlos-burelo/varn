pub const DISPOSABLE: &str = "Disposable";
pub const ASYNC_DISPOSABLE: &str = "AsyncDisposable";

pub const RUNTIME_RANGE: &str = "__range__";

/// Platform error classes the runtime itself instantiates (spec §41).
pub const ERROR: &str = "Error";
/// Reserved for `#{…}`; `Record<K, V>` is a forbidden type spelling.
pub const RECORD: &str = "Record";
pub const TYPE_ERROR: &str = "TypeError";
pub const RANGE_ERROR: &str = "RangeError";
