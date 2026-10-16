use std::path::{Path, PathBuf};

use varn_builtins::StdInput;
use varn_modules::std_root::{self, StdProvenance, StdSource};

const IMAGE_BUILD_COMMAND: &str = "std-bundle";

pub fn register() {
    let building_image = std::env::args().nth(1).as_deref() == Some(IMAGE_BUILD_COMMAND);
    if let Some(input) = select(building_image) {
        varn_builtins::register_std(input);
    }
}

fn select(building_image: bool) -> Option<StdInput> {
    let (source, provenance) = std_root::resolve();
    Some(match source {
        StdSource::Embedded => match carried_image() {
            Some(bytes) => StdInput::Image {
                bytes,
                tree_root: None,
                provenance,
            },
            None if provenance == StdProvenance::Env => StdInput::Unavailable(format!(
                "{}={} was requested but this binary has no stdlib compiled into it",
                std_root::ENV_VARN_STD,
                std_root::STD_EMBEDDED_SENTINEL
            )),
            None => return None,
        },
        StdSource::SourceTree(root) if building_image => StdInput::Tree { root, provenance },
        StdSource::SourceTree(root) => match image_for_tree(&root) {
            Ok(bytes) => StdInput::Image {
                bytes,
                tree_root: Some(root),
                provenance,
            },
            Err(reason) => StdInput::Unavailable(reason),
        },
    })
}

fn carried_image() -> Option<&'static [u8]> {
    #[cfg(feature = "embed-std")]
    {
        Some(include_bytes!("../../../dist/std.vnb"))
    }
    #[cfg(not(feature = "embed-std"))]
    {
        let bytes = std::fs::read(std::env::var_os("VARN_STD_BUNDLE")?).ok()?;
        Some(Box::leak(bytes.into_boxed_slice()))
    }
}

fn tree_stamp(root: &Path) -> u64 {
    let mut stamp = varn_modules::artifact::cache_key().to_le_bytes().to_vec();
    for m in std_root::tree_modules(root) {
        stamp.extend_from_slice(m.path.as_bytes());
        stamp.extend_from_slice(&m.len.to_le_bytes());
        stamp.extend_from_slice(&m.modified_nanos.to_le_bytes());
    }
    varn_core::fingerprint::fingerprint(&stamp)
}

fn image_for_tree(root: &Path) -> Result<&'static [u8], String> {
    let dir = varn_modules::artifact::get_cache_dir(root).join("std");
    let path = dir.join(format!("std.{:016x}.vnb", tree_stamp(root)));
    if let Some(bytes) = read_valid_image(&path) {
        return Ok(bytes);
    }
    build_image(root, &dir, &path)?;
    varn_modules::artifact::prune_superseded(&path);
    read_valid_image(&path)
        .ok_or_else(|| format!("std image built from {} is unreadable", root.display()))
}

fn read_valid_image(path: &Path) -> Option<&'static [u8]> {
    let bytes: &'static [u8] = Box::leak(std::fs::read(path).ok()?.into_boxed_slice());
    varn_modules::std_image::StdImage::parse(bytes)
        .is_ok()
        .then_some(bytes)
}

fn build_image(root: &Path, dir: &Path, path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let tmp: PathBuf = path.with_extension(format!("tmp{}", std::process::id()));
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate vn: {e}"))?;
    let output = std::process::Command::new(exe)
        .arg(IMAGE_BUILD_COMMAND)
        .arg("--std-dir")
        .arg(root)
        .arg("--out")
        .arg(&tmp)
        .env(std_root::ENV_VARN_STD, root)
        .output()
        .map_err(|e| format!("cannot run vn {IMAGE_BUILD_COMMAND}: {e}"))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "cannot build the std image from {}:\n{}",
            root.display(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot store the std image at {}: {e}", path.display())
    })
}
