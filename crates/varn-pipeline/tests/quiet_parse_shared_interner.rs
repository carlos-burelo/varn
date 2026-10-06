#![allow(unused_crate_dependencies)]

use std::fs;
use std::path::PathBuf;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "varn-task7e-fix1-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn build_module_graph_resolves_named_reexport_origin_module_without_panicking() {
    varn_pipeline::resolver::reset();

    let dir = scratch_dir("named-reexport");

    let leaf_path = dir.join("leaf.vn");
    let mid_path = dir.join("mid.vn");
    let entry_path = dir.join("entry.vn");

    fs::write(
        &leaf_path,
        "export function leafValue(): int {\n  return 42\n}\n",
    )
    .expect("write leaf.vn");

    fs::write(
        &mid_path,
        "import { leafValue } from \"./leaf.vn\"\nexport function midValue(): int {\n  return leafValue()\n}\n",
    )
    .expect("write mid.vn");

    fs::write(
        &entry_path,
        "import { midValue } from \"./mid.vn\"\nlet result: int = midValue()\n",
    )
    .expect("write entry.vn");

    let source = fs::read_to_string(&entry_path).expect("read entry.vn");
    let entry_str = entry_path.to_string_lossy().into_owned();

    let result = std::panic::catch_unwind(|| {
        varn_pipeline::compile_source_for_build(
            &source,
            &entry_str,
            false,
            &varn_pipeline::DebugFlags::default(),
        )
    });

    let _ = fs::remove_dir_all(&dir);

    match result {
        Err(_) => panic!(
            "build_module_graph panicked while re-checking a module with a named \
             re-exported import — this is the cross-interner Atom index bug \
             Task 7e's quiet_parse fix closes"
        ),
        Ok(Err(e)) => panic!("expected the module graph to compile cleanly, got: {e}"),
        Ok(Ok(_compiled)) => {}
    }
}
