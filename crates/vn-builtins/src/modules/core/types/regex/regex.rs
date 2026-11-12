use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, PoisonError, RwLock};
use vn_core::StdErrorClass;
use vn_macros::vn_contract;
use vn_types::NativeError;
use vn_types::{NativeCtx, VmValue};

const POOL_CAPACITY: usize = 4096;
const SLOT_BITS: u32 = 20;
const SLOT_MASK: i64 = (1 << SLOT_BITS) - 1;

const HANDLE_FIELD: &str = "handle";
const SOURCE_FIELD: &str = "source";
const OPTIONS_FIELD: &str = "options";

struct Entry {
    regex: Arc<regex::Regex>,
    generation: u32,
    last_used: AtomicU64,
}

struct RegexPool {
    slots: Vec<Option<Entry>>,
    by_source: HashMap<(String, String), usize>,
    generations: u32,
}

static USE_CLOCK: AtomicU64 = AtomicU64::new(0);

static REGEX_POOL: LazyLock<RwLock<RegexPool>> = LazyLock::new(|| {
    RwLock::new(RegexPool {
        slots: Vec::new(),
        by_source: HashMap::new(),
        generations: 0,
    })
});

fn touch(entry: &Entry) {
    entry
        .last_used
        .store(USE_CLOCK.fetch_add(1, Ordering::Relaxed), Ordering::Relaxed);
}

fn handle_of(slot: usize, generation: u32) -> i64 {
    (i64::from(generation) << SLOT_BITS) | slot as i64
}

pub struct RegexClass;

vn_macros::vn_record! {
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

fn lookup(handle: i64) -> Option<Arc<regex::Regex>> {
    let pool = REGEX_POOL.read().unwrap_or_else(PoisonError::into_inner);
    let entry = pool.slots.get((handle & SLOT_MASK) as usize)?.as_ref()?;
    if i64::from(entry.generation) != handle >> SLOT_BITS {
        return None;
    }
    touch(entry);
    Some(Arc::clone(&entry.regex))
}

fn free_slot(pool: &mut RegexPool) -> usize {
    if pool.slots.len() < POOL_CAPACITY {
        pool.slots.push(None);
        return pool.slots.len() - 1;
    }
    let victim = pool
        .slots
        .iter()
        .enumerate()
        .min_by_key(|(_, slot)| {
            slot.as_ref()
                .map_or(0, |e| e.last_used.load(Ordering::Relaxed))
        })
        .map(|(at, _)| at)
        .expect("a full pool has slots");
    pool.by_source.retain(|_, at| *at != victim);
    pool.slots[victim] = None;
    victim
}

fn intern(pattern: &str, flags: &str) -> Result<(i64, Arc<regex::Regex>), NativeError> {
    let key = (pattern.to_string(), flags.to_string());
    {
        let pool = REGEX_POOL.read().unwrap_or_else(PoisonError::into_inner);
        if let Some(&slot) = pool.by_source.get(&key)
            && let Some(entry) = pool.slots[slot].as_ref()
        {
            touch(entry);
            return Ok((handle_of(slot, entry.generation), Arc::clone(&entry.regex)));
        }
    }
    let compiled = Arc::new(
        vn_core::regex_pattern::compile(pattern, flags)
            .map_err(|e| NativeError::std(StdErrorClass::Parse, e.to_string()))?,
    );
    let mut pool = REGEX_POOL.write().unwrap_or_else(PoisonError::into_inner);
    if let Some(&slot) = pool.by_source.get(&key)
        && let Some(entry) = pool.slots[slot].as_ref()
    {
        touch(entry);
        return Ok((handle_of(slot, entry.generation), Arc::clone(&entry.regex)));
    }
    let slot = free_slot(&mut pool);
    pool.generations = pool.generations.wrapping_add(1).max(1);
    let generation = pool.generations;
    let entry = Entry {
        regex: Arc::clone(&compiled),
        generation,
        last_used: AtomicU64::new(0),
    };
    touch(&entry);
    pool.slots[slot] = Some(entry);
    pool.by_source.insert(key, slot);
    Ok((handle_of(slot, generation), compiled))
}

fn text_field(ctx: &dyn NativeCtx, this: VmValue, field: &str) -> String {
    ctx.get_field(this, field)
        .and_then(|v| ctx.str_owned(v))
        .expect("a Regex instance carries its source")
}

fn of_instance(ctx: &mut dyn NativeCtx, this: VmValue) -> Arc<regex::Regex> {
    let handle = ctx
        .get_field(this, HANDLE_FIELD)
        .expect("a Regex instance carries its handle");
    if let Some(regex) = lookup(ctx.as_int(handle)) {
        return regex;
    }
    let pattern = text_field(ctx, this, SOURCE_FIELD);
    let flags = text_field(ctx, this, OPTIONS_FIELD);
    let (handle, regex) =
        intern(&pattern, &flags).expect("a pattern that compiled once compiles again");
    ctx.set_field(this, HANDLE_FIELD, VmValue::from_int(handle));
    regex
}

pub(crate) fn regex_argument(
    ctx: &mut dyn NativeCtx,
    value: VmValue,
) -> Option<Arc<regex::Regex>> {
    if ctx.is_string(value) {
        return None;
    }
    ctx.get_field(value, HANDLE_FIELD)?;
    Some(of_instance(ctx, value))
}

vn_contract! {
    module: "globals",
    class: "Regex",
    contract: "src/modules/core/types/regex/regex.vn",
    impl RegexClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, pattern: &str, flags: Option<&str>) -> Result<VmValue, NativeError> {
            let flags = flags.unwrap_or("");
            let (handle, _) = intern(pattern, flags)?;
            let source = ctx.alloc_str(pattern);
            let options = ctx.alloc_str(flags);
            ctx.set_field(this, SOURCE_FIELD, source);
            ctx.set_field(this, OPTIONS_FIELD, options);
            ctx.set_field(this, HANDLE_FIELD, VmValue::from_int(handle));
            Ok(this)
        }

        fn pattern(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            text_field(ctx, this, SOURCE_FIELD)
        }

        fn flags(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            text_field(ctx, this, OPTIONS_FIELD)
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
