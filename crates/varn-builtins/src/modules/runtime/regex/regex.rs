use varn_core::StdErrorClass;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::LazyLock;
use varn_macros::varn_contract;
use varn_types::NativeError;
use varn_types::{NativeCtx, VmValue};

struct RegexPool {
    compiled: Vec<regex::Regex>,
    handles: HashMap<(String, String), i64>,
}

static REGEX_POOL: LazyLock<RwLock<RegexPool>> = LazyLock::new(|| {
    RwLock::new(RegexPool {
        compiled: Vec::new(),
        handles: HashMap::new(),
    })
});

const CASE_INSENSITIVE: char = 'i';
const MULTI_LINE: char = 'm';
const DOT_MATCHES_NEWLINE: char = 's';
const IGNORE_WHITESPACE: char = 'x';
const FLAGS: [char; 4] = [CASE_INSENSITIVE, MULTI_LINE, DOT_MATCHES_NEWLINE, IGNORE_WHITESPACE];

pub struct RegexRuntime;

varn_macros::varn_record! {
    contract: "src/modules/runtime/regex/regex_runtime.vn",
    interface: "RegexMatch",
    struct RegexMatch;
}

fn match_record(ctx: &mut dyn NativeCtx, caps: &regex::Captures<'_>) -> VmValue {
    let whole = caps.get(0).expect("a capture set always holds the whole match");
    let groups: Vec<VmValue> = caps
        .iter()
        .skip(1)
        .map(|g| g.map_or(VmValue::null(), |grp| ctx.alloc_str(grp.as_str())))
        .collect();
    let matched = ctx.alloc_str(whole.as_str());
    let groups = ctx.alloc_array(groups);
    RegexMatch {
        r#match: matched,
        index: whole.start() as i64,
        groups,
    }
    .alloc(ctx)
}

fn get_regex(handle: i64) -> Result<regex::Regex, String> {
    if handle < 0 {
        return Err("Invalid regex handle".to_string());
    }
    let pool = REGEX_POOL.read();
    pool.compiled
        .get(handle as usize)
        .cloned()
        .ok_or_else(|| format!("Regex handle {handle} not found"))
}

varn_contract! {
    module: "runtime:regex",
    contract: "src/modules/runtime/regex/regex_runtime.vn",
    impl RegexRuntime {
        fn compile(_ctx: &mut dyn NativeCtx, pattern: &str, flags: &str) -> Result<i64, NativeError> {
            if let Some(bad) = flags.chars().find(|c| !FLAGS.contains(c)) {
                return Err(NativeError::std(StdErrorClass::Parse, format!("unknown regex flag '{bad}': the flags are i, m, s and x")));
            }
            let key = (pattern.to_string(), flags.to_string());
            if let Some(&handle) = REGEX_POOL.read().handles.get(&key) {
                return Ok(handle);
            }
            let mut builder = regex::RegexBuilder::new(pattern);
            builder
                .case_insensitive(flags.contains(CASE_INSENSITIVE))
                .multi_line(flags.contains(MULTI_LINE))
                .dot_matches_new_line(flags.contains(DOT_MATCHES_NEWLINE))
                .ignore_whitespace(flags.contains(IGNORE_WHITESPACE));
            let re = builder.build().map_err(|e| format!("Invalid regex pattern: {e}")).map_err(|e| NativeError::std(StdErrorClass::Parse, e))?;
            let mut pool = REGEX_POOL.write();
            if let Some(&handle) = pool.handles.get(&key) {
                return Ok(handle);
            }
            let handle = pool.compiled.len() as i64;
            pool.compiled.push(re);
            pool.handles.insert(key, handle);
            Ok(handle)
        }

        fn test(_ctx: &mut dyn NativeCtx, handle: i64, text: &str) -> Result<bool, NativeError> {
            let re = get_regex(handle).map_err(|e| NativeError::std(StdErrorClass::Parse, e))?;
            Ok(re.is_match(text))
        }

        fn exec(ctx: &mut dyn NativeCtx, handle: i64, text: &str) -> Result<Option<VmValue>, NativeError> {
            let re = get_regex(handle).map_err(|e| NativeError::std(StdErrorClass::Parse, e))?;
            Ok(re.captures(text).map(|caps| match_record(ctx, &caps)))
        }

        fn findAll(ctx: &mut dyn NativeCtx, handle: i64, text: &str) -> Result<Vec<VmValue>, NativeError> {
            let re = get_regex(handle).map_err(|e| NativeError::std(StdErrorClass::Parse, e))?;
            Ok(re
                .captures_iter(text)
                .map(|caps| match_record(ctx, &caps))
                .collect())
        }

        fn replace(_ctx: &mut dyn NativeCtx, handle: i64, text: &str, replacement: &str) -> Result<String, NativeError> {
            let re = get_regex(handle).map_err(|e| NativeError::std(StdErrorClass::Parse, e))?;
            Ok(re.replace_all(text, replacement).into_owned())
        }

        fn split(ctx: &mut dyn NativeCtx, handle: i64, text: &str) -> Result<Vec<VmValue>, NativeError> {
            let re = get_regex(handle).map_err(|e| NativeError::std(StdErrorClass::Parse, e))?;
            Ok(re.split(text).map(|p| ctx.alloc_str(p)).collect())
        }
    }
}
