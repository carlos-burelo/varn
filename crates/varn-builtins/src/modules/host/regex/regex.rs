use parking_lot::RwLock;
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, Value, VmValue};

static REGEX_POOL: RwLock<Vec<regex::Regex>> = RwLock::new(Vec::new());

pub struct RegexRuntime;

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
    contract: "src/modules/host/regex/regex_runtime.vn",
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

        fn exec(ctx: &mut dyn NativeCtx, handle: i64, text: &str) -> Result<VmValue, String> {
            let re = get_regex(handle)?;
            if let Some(caps) = re.captures(text) {
                let m = caps.get(0).unwrap();
                let obj = ctx.alloc_object();

                let match_str = ctx.intern(Value::Str(m.as_str().into()));
                ctx.set_field(obj, "match", match_str);

                let index_val = VmValue::from_int(m.start() as i64);
                ctx.set_field(obj, "index", index_val);

                let groups_vec: Vec<VmValue> = caps
                    .iter()
                    .skip(1)
                    .map(|g| {
                        if let Some(grp) = g {
                            ctx.intern(Value::Str(grp.as_str().into()))
                        } else {
                            VmValue::null()
                        }
                    })
                    .collect();

                let groups_nv = ctx.alloc_array(groups_vec);
                ctx.set_field(obj, "groups", groups_nv);

                Ok(obj)
            } else {
                Ok(VmValue::null())
            }
        }

        fn findAll(ctx: &mut dyn NativeCtx, handle: i64, text: &str) -> Result<VmValue, String> {
            let re = get_regex(handle)?;
            let mut results = Vec::new();

            for caps in re.captures_iter(text) {
                let m = caps.get(0).unwrap();
                let obj = ctx.alloc_object();

                let match_str = ctx.intern(Value::Str(m.as_str().into()));
                ctx.set_field(obj, "match", match_str);

                let index_val = VmValue::from_int(m.start() as i64);
                ctx.set_field(obj, "index", index_val);

                let groups_vec: Vec<VmValue> = caps
                    .iter()
                    .skip(1)
                    .map(|g| {
                        if let Some(grp) = g {
                            ctx.intern(Value::Str(grp.as_str().into()))
                        } else {
                            VmValue::null()
                        }
                    })
                    .collect();

                let groups_nv = ctx.alloc_array(groups_vec);
                ctx.set_field(obj, "groups", groups_nv);

                results.push(obj);
            }

            Ok(ctx.alloc_array(results))
        }

        fn replace(_ctx: &mut dyn NativeCtx, handle: i64, text: &str, replacement: &str) -> Result<String, String> {
            let re = get_regex(handle)?;
            Ok(re.replace_all(text, replacement).into_owned())
        }

        fn split(ctx: &mut dyn NativeCtx, handle: i64, text: &str) -> Result<VmValue, String> {
            let re = get_regex(handle)?;
            let parts: Vec<VmValue> = re
                .split(text)
                .map(|p| ctx.intern(Value::Str(p.into())))
                .collect();
            Ok(ctx.alloc_array(parts))
        }
    }
}
