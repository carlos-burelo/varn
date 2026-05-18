use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::manifest::DepOrigin;

pub fn fetch_and_extract(
    origin: &DepOrigin,
    version: &str,
    dest_dir: &Path,
) -> Result<String, String> {
    let url = origin.tarball_url(version);

    let response = ureq::get(&url)
        .set("User-Agent", "varn-pm/0.1")
        .call()
        .map_err(|e| format!("cannot download {url}: {e}"))?;

    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read download body: {e}"))?;

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let integrity = format!("sha256:{}", hex::encode(digest));

    if dest_dir.exists() {
        std::fs::remove_dir_all(dest_dir)
            .map_err(|e| format!("cannot clear {}: {e}", dest_dir.display()))?;
    }
    std::fs::create_dir_all(dest_dir)
        .map_err(|e| format!("cannot create {}: {e}", dest_dir.display()))?;

    extract_tarball(&bytes, dest_dir, version)?;

    Ok(integrity)
}

pub fn verify_cached(dest_dir: &Path, expected_integrity: &str) -> bool {
    if !dest_dir.exists() {
        return false;
    }

    let integrity_file = dest_dir.join(".integrity");
    if let Ok(stored) = std::fs::read_to_string(&integrity_file) {
        return stored.trim() == expected_integrity;
    }
    false
}

pub fn write_integrity_file(dest_dir: &Path, integrity: &str) -> Result<(), String> {
    std::fs::write(dest_dir.join(".integrity"), integrity)
        .map_err(|e| format!("cannot write .integrity: {e}"))
}

fn extract_tarball(bytes: &[u8], dest_dir: &Path, _version: &str) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let decoder = GzDecoder::new(bytes);
    let mut archive = Archive::new(decoder);

    let strip_prefix_len: usize = 1;

    for entry in archive.entries().map_err(|e| format!("bad tarball: {e}"))? {
        let mut entry = entry.map_err(|e| format!("bad tar entry: {e}"))?;
        let entry_path = entry.path().map_err(|e| format!("bad entry path: {e}"))?;

        let components: Vec<_> = entry_path.components().collect();
        if components.len() <= strip_prefix_len {
            continue;
        }
        let relative: PathBuf = components[strip_prefix_len..].iter().collect();
        let out_path = dest_dir.join(&relative);

        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| format!("cannot create dir {}: {e}", out_path.display()))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create dir {}: {e}", parent.display()))?;
            }
            let mut file = std::fs::File::create(&out_path)
                .map_err(|e| format!("cannot create {}: {e}", out_path.display()))?;
            std::io::copy(&mut entry, &mut file)
                .map_err(|e| format!("cannot write {}: {e}", out_path.display()))?;
        }
    }

    Ok(())
}
