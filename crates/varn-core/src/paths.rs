use crate::env::settings;
use std::env::current_dir;
use std::path::PathBuf;

pub fn varn_home_dir() -> PathBuf {
    let settings = settings();
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

pub fn varn_cache_dir() -> PathBuf {
    if let Some(dir) = &settings().cache_dir {
        return dir.clone();
    }
    varn_home_dir().join("cache")
}
