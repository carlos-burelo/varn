use std::path::PathBuf;

use varn_modules::artifact::{self, ArtifactClass, ArtifactKind};

const SIZE_LIMIT: u64 = 256 << 20;

struct DiskCodeStore {
    dir: PathBuf,
    pruned: std::sync::Once,
}

impl DiskCodeStore {
    fn path(&self, key: u64) -> PathBuf {
        self.dir.join(format!("{key:016x}.vnj"))
    }

    fn prune(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((meta.modified().ok()?, meta.len(), e.path()))
            })
            .collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        if total <= SIZE_LIMIT {
            return;
        }
        files.sort_by_key(|f| f.0);
        for (_, len, path) in files {
            if total <= SIZE_LIMIT / 2 {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total -= len;
            }
        }
    }
}

impl varn_vm::jit::code_cache::CodeStore for DiskCodeStore {
    fn load(&self, key: u64) -> Option<Vec<u8>> {
        let bytes = std::fs::read(self.path(key)).ok()?;
        artifact::read_artifact(ArtifactKind::JitCode, &bytes)
            .ok()
            .map(<[u8]>::to_vec)
    }

    fn store(&self, key: u64, payload: &[u8]) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        self.pruned.call_once(|| self.prune());
        let bytes = artifact::write_artifact(ArtifactKind::JitCode, ArtifactClass::Cache, payload);
        let _ = artifact::write_artifact_file(&self.path(key), &bytes);
    }
}

pub fn register() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let dir = artifact::get_cache_dir(&cwd).join("jit");
    varn_vm::jit::code_cache::register_code_store(Box::leak(Box::new(DiskCodeStore {
        dir,
        pruned: std::sync::Once::new(),
    })));
}
