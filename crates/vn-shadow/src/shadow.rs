use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const SHADOW_DIR: &str = ".vn-shadow";

pub fn materialize(target: &Path) -> io::Result<PathBuf> {
    let meta = fs::metadata(target)?;
    let stamp = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let key = format!("{:x}-{:x}", meta.len(), stamp);
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("target has no parent directory"))?;
    let file_name = target
        .file_name()
        .ok_or_else(|| io::Error::other("target has no file name"))?;
    let root = parent.join(SHADOW_DIR);
    let slot = root.join(&key);
    let image = slot.join(file_name);
    if !has_len(&image, meta.len()) {
        fs::create_dir_all(&slot)?;
        let mut staging_name = file_name.to_os_string();
        staging_name.push(format!(".{}.partial", std::process::id()));
        let staging = slot.join(staging_name);
        fs::copy(target, &staging)?;
        if let Err(e) = fs::rename(&staging, &image) {
            let _ = fs::remove_file(&staging);
            if !has_len(&image, meta.len()) {
                return Err(e);
            }
        }
    }
    prune_except(&root, &key);
    Ok(image)
}

fn has_len(path: &Path, len: u64) -> bool {
    fs::metadata(path).map(|m| m.len() == len).unwrap_or(false)
}

fn prune_except(root: &Path, live: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() != live {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
