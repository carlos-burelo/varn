use std::path::{Path, PathBuf};

use crate::manifest::{Manifest, ManifestError};
use crate::root::{locate, Scope};

pub enum Anchor<'a> {
    File(&'a Path),
    Dir(&'a Path),
    Cwd,
}

#[derive(Clone)]
pub struct Project {
    pub root: PathBuf,
    pub manifest_path: Option<PathBuf>,
    pub manifest: Manifest,
}

impl Project {
    pub fn detached() -> Project {
        Project {
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            manifest_path: None,
            manifest: Manifest::default(),
        }
    }

    pub fn discover(anchor: Anchor<'_>) -> Result<Project, ManifestError> {
        Project::find(anchor, Scope::Nearest)
    }

    pub fn owning(anchor: Anchor<'_>) -> Result<Project, ManifestError> {
        Project::find(anchor, Scope::Owner)
    }

    fn find(anchor: Anchor<'_>, scope: Scope) -> Result<Project, ManifestError> {
        let start = match anchor {
            Anchor::File(file) => std::path::absolute(file)
                .unwrap_or_else(|_| file.to_path_buf())
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
            Anchor::Dir(dir) => dir.to_path_buf(),
            Anchor::Cwd => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        };
        let located = locate(&start, scope);
        let manifest = match &located.manifest {
            Some(path) => Manifest::load(path)?,
            None => Manifest::default(),
        };
        Ok(Project {
            root: located.dir,
            manifest_path: located.manifest,
            manifest,
        })
    }
}
