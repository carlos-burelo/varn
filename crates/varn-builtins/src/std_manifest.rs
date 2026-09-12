//! std.json manifest of a std source tree.

use std::path::Path;

#[derive(serde::Deserialize)]
pub struct StdManifest {
    pub version: String,
    #[serde(rename = "hostApi")]
    pub host_api: u32,
    pub modules: Vec<StdManifestModule>,
}

#[derive(serde::Deserialize)]
pub struct StdManifestModule {
    pub id: String,
    #[serde(default)]
    pub pure: bool,
}

pub fn read_manifest(root: &Path) -> Option<StdManifest> {
    if let Ok(raw) = std::fs::read_to_string(root.join("std.json")) {
        match serde_json::from_str(&raw) {
            Ok(m) => return Some(m),
            Err(e) => panic!("invalid std.json in {}: {e}", root.display()),
        }
    }
    scan_std_tree(root)
}

fn scan_std_tree(root: &Path) -> Option<StdManifest> {
    let mut files = Vec::new();
    collect_vn_files(root, root, &mut files);
    if files.is_empty() {
        return None;
    }
    files.sort();
    let mut modules = Vec::new();
    for rel_path in files {
        let (id, _is_mod) = if rel_path.ends_with("/mod.vn") {
            let prefix = rel_path.strip_suffix("/mod.vn").unwrap();
            (format!("std:{prefix}"), true)
        } else if rel_path.ends_with(".vn") {
            let prefix = rel_path.strip_suffix(".vn").unwrap();
            (format!("std:{prefix}"), false)
        } else {
            continue;
        };
        // If it's a file `foo.vn` but `foo/mod.vn` also exists, `mod.vn` represents `std:foo`.
        let full_path = root.join(&rel_path);
        let pure = if let Ok(source) = std::fs::read_to_string(&full_path) {
            !source.contains("\"runtime:") && !source.contains("'runtime:")
        } else {
            false
        };
        modules.push(StdManifestModule { id, pure });
    }
    Some(StdManifest {
        version: "0.3.0".to_string(),
        host_api: varn_core::HOST_API_VERSION,
        modules,
    })
}

fn collect_vn_files(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_vn_files(base, &p, out);
        } else if p.extension().is_some_and(|ext| ext == "vn") {
            if let Ok(rel) = p.strip_prefix(base) {
                let normalized = rel.to_string_lossy().replace('\\', "/");
                out.push(normalized);
            }
        }
    }
}
