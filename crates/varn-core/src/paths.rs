use std::env::{current_dir, current_exe, var};
use std::path::PathBuf;

pub fn varn_home_dir() -> PathBuf {
    if let Ok(raw) = var("VARN_HOME") {
        let p = PathBuf::from(raw);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }

    if let Some(home) = user_home_dir() {
        return home.join(".vn");
    }

    current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".vn")
}

pub fn varn_cache_dir() -> PathBuf {
    if let Ok(raw) = var("VARN_CACHE_DIR") {
        let p = PathBuf::from(raw);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    varn_home_dir().join("cache")
}

pub fn stdlib_candidates() -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(8);

    if let Ok(raw) = var("VARN_STDLIB") {
        let p = PathBuf::from(raw);
        if !p.as_os_str().is_empty() {
            out.push(p);
        }
    }

    out.push(varn_home_dir().join("stdlib"));

    if let Ok(cwd) = current_dir() {
        out.push(cwd.join("varn-stdlib"));
        if let Some(parent) = cwd.parent() {
            out.push(parent.join("varn-stdlib"));
        }
    }

    if let Ok(exe) = current_exe() {
        if let Some(exe_dir) = exe.parent() {
            out.push(exe_dir.join("varn-stdlib"));
            out.push(exe_dir.join("../share/Varn/stdlib"));
        }
    }

    out
}

fn user_home_dir() -> Option<PathBuf> {
    if let Ok(home) = var("HOME") {
        let p = PathBuf::from(home);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }

    if let Ok(user_profile) = var("USERPROFILE") {
        let p = PathBuf::from(user_profile);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }

    let drive = var("HOMEDRIVE").ok();
    let path = var("HOMEPATH").ok();
    match (drive, path) {
        (Some(d), Some(p)) if !d.is_empty() && !p.is_empty() => Some(PathBuf::from(d).join(p)),
        _ => None,
    }
}
