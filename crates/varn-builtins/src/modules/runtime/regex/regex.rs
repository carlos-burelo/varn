use parking_lot::RwLock;
use varn_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

static REGEX_POOL: RwLock<Vec<regex::Regex>> = RwLock::new(Vec::new());

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
    pool.get(handle as usize)
        .cloned()
        .ok_or_else(|| format!("Regex handle {handle} not found"))
}

varn_contract! {
    module: "runtime:regex",
    contract: "src/modules/runtime/regex/regex_runtime.vn",
    impl RegexRuntime {
        fn compile(_ctx: &mut dyn NativeCtx, pattern: &str, flags: &str) -> Result<i64, String> {
            let mut builder = regex::RegexBuilder::new(pattern);
            if flags.contains('i') {
                builder.case_insensitive(true);
            }
            if flags.contains('m') {
                builder.multi_line(true);
            }
            if flags.contains('s') {
                builder.dot_matches_new_line(true);
            }
            if flags.contains('x') {
                builder.ignore_whitespace(true);
            }

            let re = builder.build().map_err(|e| format!("Invalid regex pattern: {e}"))?;
            let mut pool = REGEX_POOL.write();
            let handle = pool.len() as i64;
            pool.push(re);
            Ok(handle)
        }

        fn test(_ctx: &mut dyn NativeCtx, handle: i64, text: &str) -> Result<bool, String> {
            let re = get_regex(handle)?;
            Ok(re.is_match(text))
        }

        fn exec(ctx: &mut dyn NativeCtx, handle: i64, text: &str) -> Result<Option<VmValue>, String> {
            let re = get_regex(handle)?;
            Ok(re.captures(text).map(|caps| match_record(ctx, &caps)))
        }

        fn findAll(ctx: &mut dyn NativeCtx, handle: i64, text: &str) -> Result<Vec<VmValue>, String> {
            let re = get_regex(handle)?;
            Ok(re
                .captures_iter(text)
                .map(|caps| match_record(ctx, &caps))
                .collect())
        }

        fn replace(_ctx: &mut dyn NativeCtx, handle: i64, text: &str, replacement: &str) -> Result<String, String> {
            let re = get_regex(handle)?;
            Ok(re.replace_all(text, replacement).into_owned())
        }

        fn split(ctx: &mut dyn NativeCtx, handle: i64, text: &str) -> Result<Vec<VmValue>, String> {
            let re = get_regex(handle)?;
            Ok(re.split(text).map(|p| ctx.alloc_str(p)).collect())
        }
    }
}
