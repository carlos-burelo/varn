use varn_contract::varn_contract;
use varn_types::{NativeCtx, VnArray};

pub struct PathRuntime;

fn main_sep() -> char {
    std::path::MAIN_SEPARATOR
}

fn collect(v: &[char]) -> String {
    v.iter().collect()
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

fn normalize_inner(p: &str, sep: char) -> String {
    if p.is_empty() {
        return ".".to_string();
    }
    let c: Vec<char> = p.chars().collect();
    let mut drive = String::new();
    let mut rest: Vec<char> = c.clone();
    if c.len() >= 2 && c[1] == ':' {
        drive = collect(&c[..2]);
        rest = c[2..].to_vec();
    }
    let is_abs = rest.first().is_some_and(|&x| is_sep(x));
    let normalized: String = rest
        .iter()
        .map(|&x| if x == '\\' { '/' } else { x })
        .collect();
    let mut segments: Vec<String> = Vec::new();
    for seg in normalized.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            if !segments.is_empty() && segments.last().is_some_and(|l| l != "..") {
                segments.pop();
            } else if !is_abs {
                segments.push("..".to_string());
            }
        } else {
            segments.push(seg.to_string());
        }
    }
    let mut resolved = String::new();
    for (i, s) in segments.iter().enumerate() {
        if i > 0 {
            resolved.push(sep);
        }
        resolved.push_str(s);
    }
    if is_abs {
        if !drive.is_empty() {
            return format!("{drive}{sep}{resolved}");
        }
        return format!("{sep}{resolved}");
    }
    if resolved.is_empty() {
        if !drive.is_empty() {
            return drive;
        }
        return ".".to_string();
    }
    if !drive.is_empty() {
        return format!("{drive}{resolved}");
    }
    resolved
}

fn dirname_inner(p: &str) -> String {
    let c: Vec<char> = p.chars().collect();
    if c.is_empty() {
        return ".".to_string();
    }
    let mut end = c.len();
    loop {
        if end > 0 {
            if is_sep(c[end - 1]) {
                end -= 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    if end == 0 {
        return c[0].to_string();
    }
    if end == 2 && c[1] == ':' {
        return collect(&c);
    }
    let mut i = end as i64 - 1;
    let mut found: i64 = -1;
    loop {
        if i >= 0 {
            if is_sep(c[i as usize]) {
                found = i;
                break;
            }
            i -= 1;
        } else {
            break;
        }
    }
    if found == -1 {
        if c.len() >= 2 && c[1] == ':' {
            return collect(&c[..2]);
        }
        return ".".to_string();
    }
    if found == 0 {
        return c[0].to_string();
    }
    if found == 2 && c[1] == ':' {
        return collect(&c[..3]);
    }
    let mut dir_end = found as usize;
    loop {
        if dir_end > 0 {
            if is_sep(c[dir_end - 1]) {
                dir_end -= 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    if dir_end == 0 {
        return c[0].to_string();
    }
    if dir_end == 2 && c[1] == ':' {
        return collect(&c[..2]);
    }
    collect(&c[..dir_end])
}

fn basename_inner(p: &str, ext: Option<&str>) -> String {
    let c: Vec<char> = p.chars().collect();
    if c.is_empty() {
        return String::new();
    }
    let mut end = c.len();
    loop {
        if end > 0 {
            if is_sep(c[end - 1]) {
                end -= 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    if end == 2 && c[1] == ':' {
        return String::new();
    }
    if end == 0 {
        return String::new();
    }
    let mut start = 0;
    let mut i = end as i64 - 1;
    loop {
        if i >= 0 {
            if is_sep(c[i as usize]) {
                start = (i + 1) as usize;
                break;
            }
            i -= 1;
        } else {
            break;
        }
    }
    let base: String = collect(&c[start..end]);
    if let Some(e) = ext {
        let base_n = base.chars().count();
        let ext_n = e.chars().count();
        if base_n >= ext_n && base.ends_with(e) {
            return base.chars().take(base_n - ext_n).collect();
        }
    }
    base
}

fn extname_inner(p: &str) -> String {
    let base = basename_inner(p, None);
    let bc: Vec<char> = base.chars().collect();
    if let Some(idx) = bc.iter().rposition(|&x| x == '.') {
        return collect(&bc[idx..]);
    }
    String::new()
}

fn is_absolute_inner(p: &str) -> bool {
    let c: Vec<char> = p.chars().collect();
    !c.is_empty() && (is_sep(c[0]) || (c.len() >= 2 && c[1] == ':'))
}

fn join_inner(segs: &[String], sep: char) -> String {
    let mut out = String::new();
    for s in segs {
        if s.is_empty() {
            continue;
        }
        if !out.is_empty() && !out.ends_with(sep) && !s.starts_with(sep) {
            out.push(sep);
            out.push_str(s);
        } else {
            out.push_str(s);
        }
    }
    normalize_inner(&out, sep)
}

fn resolve_inner(segs: &[String], sep: char, cwd: &str) -> String {
    let mut resolved = String::new();
    for seg in segs.iter().rev() {
        if seg.is_empty() {
            continue;
        }
        if resolved.is_empty() {
            resolved = seg.clone();
        } else if !seg.ends_with(sep) && !resolved.starts_with(sep) {
            resolved = format!("{seg}{sep}{resolved}");
        } else {
            resolved = format!("{seg}{resolved}");
        }
        if is_absolute_inner(&resolved) {
            return normalize_inner(&resolved, sep);
        }
    }
    if resolved.is_empty() {
        return cwd.to_string();
    }
    if !cwd.ends_with(sep) && !resolved.starts_with(sep) {
        return normalize_inner(&format!("{cwd}{sep}{resolved}"), sep);
    }
    normalize_inner(&format!("{cwd}{resolved}"), sep)
}

fn coded(code: &str, msg: impl std::fmt::Display) -> String {
    format!("{code}|{msg}")
}

fn str_array(ctx: &mut dyn NativeCtx, arr: VnArray) -> Result<Vec<String>, String> {
    let len = arr.len(ctx);
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        match arr.get(ctx, i).and_then(|v| ctx.str_owned(v)) {
            Some(s) => out.push(s),
            None => return Err(coded("E_PATH_BAD_ARG", "expected str[]")),
        }
    }
    Ok(out)
}

varn_contract! {
    module: "runtime:path",
    contract: "src/modules/runtime/path/path_runtime.vn",
    impl PathRuntime {
        fn sep(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(main_sep().to_string())
        }

        fn delimiter(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            if cfg!(windows) {
                Ok(";".to_string())
            } else {
                Ok(":".to_string())
            }
        }

        fn normalize(_ctx: &mut dyn NativeCtx, p: &str) -> Result<String, String> {
            Ok(normalize_inner(p, main_sep()))
        }

        fn dirname(_ctx: &mut dyn NativeCtx, p: &str) -> Result<String, String> {
            Ok(dirname_inner(p))
        }

        fn basename(
            _ctx: &mut dyn NativeCtx,
            p: &str,
            ext: Option<&str>,
        ) -> Result<String, String> {
            Ok(basename_inner(p, ext))
        }

        fn extname(_ctx: &mut dyn NativeCtx, p: &str) -> Result<String, String> {
            Ok(extname_inner(p))
        }

        fn isAbsolute(_ctx: &mut dyn NativeCtx, p: &str) -> Result<bool, String> {
            Ok(is_absolute_inner(p))
        }

        fn join(ctx: &mut dyn NativeCtx, segments: VnArray) -> Result<String, String> {
            let segs = str_array(ctx, segments)?;
            Ok(join_inner(&segs, main_sep()))
        }

        fn resolve(ctx: &mut dyn NativeCtx, segments: VnArray) -> Result<String, String> {
            let segs = str_array(ctx, segments)?;
            let cwd = std::env::current_dir()
                .map_err(|e| coded("E_PATH_IO", e))?
                .to_string_lossy()
                .into_owned();
            Ok(resolve_inner(&segs, main_sep(), &cwd))
        }
    }
}
