use crate::env::process;
use std::env::current_dir;
use std::path::PathBuf;

pub fn vn_home_dir() -> PathBuf {
    let settings = process();
    if let Some(home) = &settings.home {
        return home.clone();
    }
    if let Some(home) = &settings.user_home {
        return home.join(".vn");
    }
    current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".vn")
}

pub fn vn_cache_dir() -> PathBuf {
    if let Some(dir) = &process().cache_dir {
        return dir.clone();
    }
    vn_home_dir().join("cache")
}
