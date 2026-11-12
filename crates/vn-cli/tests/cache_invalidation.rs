#![allow(unused_crate_dependencies)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Project {
    root: PathBuf,
    cache: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let root =
            std::env::temp_dir().join(format!("vn-cache-{name}-{}-{stamp}", std::process::id()));
        let cache = root.join(".cache");
        std::fs::create_dir_all(&root).expect("project dir");
        std::fs::create_dir_all(&cache).expect("cache dir");
        Self { root, cache }
    }

    fn write(&self, file: &str, source: &str) {
        std::fs::write(self.root.join(file), source).expect("write source");
    }

    fn run(&self, entry: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_vn"))
            .current_dir(&self.root)
            .arg("run")
            .arg(entry)
            .env(vn_core::env::EnvVar::CacheDir.name(), &self.cache)
            .env(vn_core::env::EnvVar::Std.name(), self.root.join("std"))
            .env("NO_COLOR", "1")
            .output()
            .expect("spawn vn")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_ok(output: &Output, expected: &str) {
    let shown = text(output);
    assert!(output.status.success(), "expected success:\n{shown}");
    assert!(shown.contains(expected), "expected {expected:?}:\n{shown}");
}

fn assert_rejected(output: &Output, token: &str) {
    let shown = text(output);
    assert!(!output.status.success(), "expected failure:\n{shown}");
    assert!(shown.contains(token), "expected {token}:\n{shown}");
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create dir");
    for entry in std::fs::read_dir(from).expect("read dir").flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}

fn std_chain(project: &Project, value_source: &str) {
    let std = project.root.join("std");
    if !std.exists() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
        copy_tree(&repo, &std);
    }
    std::fs::write(std.join("zz_c.vn"), value_source).expect("write c");
    std::fs::write(
        std.join("zz_b.vn"),
        "import { value } from \"std:zz_c\"\nexport const held = value()\n",
    )
    .expect("write b");
    std::fs::write(
        std.join("zz_a.vn"),
        "import { held } from \"std:zz_b\"\nexport const relayed = held\n",
    )
    .expect("write a");
    project.write(
        "main.vn",
        "import { relayed } from \"std:zz_a\"\nconst n: int = relayed\nprint(n)\n",
    );
}

#[test]
fn a_std_change_three_imports_away_invalidates_every_cached_interface() {
    let project = Project::new("std-transitive");
    std_chain(&project, "export function value(): int { return 1 }\n");
    assert_ok(&project.run("main.vn"), "1");
    assert_ok(&project.run("main.vn"), "1");

    std_chain(&project, "export function value(): str { return \"x\" }\n");
    assert_rejected(&project.run("main.vn"), "VN3001");
}

#[test]
fn a_std_file_rewritten_with_the_same_length_and_time_is_still_seen() {
    let project = Project::new("std-same-stamp");
    std_chain(&project, "export function value(): int { return 100 }\n");
    assert_ok(&project.run("main.vn"), "100");

    let c = project.root.join("std").join("zz_c.vn");
    let before = std::fs::metadata(&c)
        .and_then(|m| m.modified())
        .expect("mtime");
    std_chain(&project, "export function value(): str { return \"a\" }\n");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&c)
        .and_then(|file| file.set_modified(before))
        .expect("restore mtime");
    assert_eq!(
        std::fs::metadata(&c).expect("metadata").len(),
        "export function value(): int { return 100 }\n".len() as u64
    );
    assert_rejected(&project.run("main.vn"), "VN3001");
}
