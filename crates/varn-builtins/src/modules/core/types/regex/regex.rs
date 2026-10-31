use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::LazyLock;
use varn_core::StdErrorClass;
use varn_macros::varn_contract;
use varn_types::NativeError;
use varn_types::{NativeCtx, VmValue};

struct RegexPool {
    compiled: Vec<regex::Regex>,
    sources: Vec<(String, String)>,
    handles: HashMap<(String, String), i64>,
}

static REGEX_POOL: LazyLock<RwLock<RegexPool>> = LazyLock::new(|| {
    RwLock::new(RegexPool {
        compiled: Vec::new(),
        sources: Vec::new(),
        handles: HashMap::new(),
    })
});

const HANDLE_FIELD: &str = "handle";

pub struct RegexClass;

varn_macros::varn_record! {
    contract: "src/modules/core/types/regex/regex.vn",
    interface: "RegexMatch",
    struct RegexMatch;
}

struct CharIndex<'a> {
    text: &'a str,
    ascii: bool,
    byte: usize,
    chars: usize,
}

impl<'a> CharIndex<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            ascii: text.is_ascii(),
            byte: 0,
            chars: 0,
        }
    }

    fn at(&mut self, byte: usize) -> i64 {
        if self.ascii {
            return byte as i64;
        }
        if byte < self.byte {
            self.byte = 0;
            self.chars = 0;
        }
        self.chars += self.text[self.byte..byte].chars().count();
        self.byte = byte;
        self.chars as i64
    }
}

fn match_record(
    ctx: &mut dyn NativeCtx,
    caps: &regex::Captures<'_>,
    index: &mut CharIndex<'_>,
) -> VmValue {
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
        index: index.at(whole.start()),
        groups,
    }
    .alloc(ctx)
}

fn pooled(handle: i64) -> regex::Regex {
    REGEX_POOL
        .read()
        .compiled
        .get(usize::try_from(handle).expect("a Regex handle is a pool index"))
        .cloned()
        .expect("every Regex instance holds a handle its constructor interned")
}

fn intern(pattern: &str, flags: &str) -> Result<i64, NativeError> {
    let key = (pattern.to_string(), flags.to_string());
    if let Some(&handle) = REGEX_POOL.read().handles.get(&key) {
        return Ok(handle);
    }
    let re = varn_core::regex_pattern::compile(pattern, flags)
        .map_err(|e| NativeError::std(StdErrorClass::Parse, e.to_string()))?;
    let mut pool = REGEX_POOL.write();
    if let Some(&handle) = pool.handles.get(&key) {
        return Ok(handle);
    }
    let handle = pool.compiled.len() as i64;
    pool.compiled.push(re);
    pool.sources.push(key.clone());
    pool.handles.insert(key, handle);
    Ok(handle)
}

fn handle_of(ctx: &dyn NativeCtx, this: VmValue) -> i64 {
    let handle = ctx
        .get_field(this, HANDLE_FIELD)
        .expect("a Regex instance carries its handle");
    ctx.as_int(handle)
}

fn of_instance(ctx: &dyn NativeCtx, this: VmValue) -> regex::Regex {
    pooled(handle_of(ctx, this))
}

fn source_of(ctx: &dyn NativeCtx, this: VmValue) -> (String, String) {
    let at = usize::try_from(handle_of(ctx, this)).expect("a Regex handle is a pool index");
    REGEX_POOL.read().sources[at].clone()
}

pub(crate) fn regex_argument(ctx: &dyn NativeCtx, value: VmValue) -> Option<regex::Regex> {
    if ctx.is_string(value) {
        return None;
    }
    ctx.get_field(value, HANDLE_FIELD)
        .map(|handle| pooled(ctx.as_int(handle)))
}

varn_contract! {
    module: "globals",
    class: "Regex",
    contract: "src/modules/core/types/regex/regex.vn",
    impl RegexClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, pattern: &str, flags: Option<&str>) -> Result<VmValue, NativeError> {
            let flags = flags.unwrap_or("");
            let handle = intern(pattern, flags)?;
            ctx.set_field(this, HANDLE_FIELD, VmValue::from_int(handle));
            Ok(this)
        }

        fn pattern(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            source_of(ctx, this).0
        }

        fn flags(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            source_of(ctx, this).1
        }

        fn test(ctx: &mut dyn NativeCtx, this: VmValue, text: &str) -> bool {
            of_instance(ctx, this).is_match(text)
        }

        fn exec(ctx: &mut dyn NativeCtx, this: VmValue, text: &str) -> Option<VmValue> {
            let re = of_instance(ctx, this);
            let mut index = CharIndex::new(text);
            re.captures(text).map(|caps| match_record(ctx, &caps, &mut index))
        }

        fn findAll(ctx: &mut dyn NativeCtx, this: VmValue, text: &str) -> Vec<VmValue> {
            let re = of_instance(ctx, this);
            let mut index = CharIndex::new(text);
            re.captures_iter(text)
                .map(|caps| match_record(ctx, &caps, &mut index))
                .collect()
        }

        fn replace(ctx: &mut dyn NativeCtx, this: VmValue, text: &str, replacement: &str) -> String {
            of_instance(ctx, this).replacen(text, 1, replacement).into_owned()
        }

        fn replaceAll(ctx: &mut dyn NativeCtx, this: VmValue, text: &str, replacement: &str) -> String {
            of_instance(ctx, this).replace_all(text, replacement).into_owned()
        }

        fn split(ctx: &mut dyn NativeCtx, this: VmValue, text: &str) -> Vec<VmValue> {
            let re = of_instance(ctx, this);
            re.split(text).map(|p| ctx.alloc_str(p)).collect()
        }
    }
}
