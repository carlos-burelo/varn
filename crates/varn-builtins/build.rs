use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/modules/std");

    let std_dir = Path::new("src/modules/std");
    if std_dir.exists() {
        audit_modules(std_dir);
    }
}

fn audit_modules(dir: &Path) {
    let entries = fs::read_dir(dir).unwrap();
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            let mod_file = path.join("mod.rs");
            let name = path.file_name().unwrap().to_str().unwrap();
            let rs_file = path.join(format!("{}.rs", name));

            let target = if mod_file.exists() {
                Some(mod_file)
            } else if rs_file.exists() {
                Some(rs_file)
            } else {
                None
            };

            let _ = target;
        }
    }
}
