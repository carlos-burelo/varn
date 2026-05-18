use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Parsed dependency origin: `github.com/user/repo@^1.2.3`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepOrigin {
    pub host: String,
    pub user: String,
    pub repo: String,
    pub req: String, // semver requirement string, e.g. "^1.2.3"
}

impl DepOrigin {
    /// Parse `github.com/user/repo@^1.2.3` or `github.com/user/repo` (no constraint = `*`)
    pub fn parse(s: &str) -> Result<Self, String> {
        let (host_path, req) = if let Some(at) = s.rfind('@') {
            (&s[..at], s[at + 1..].to_owned())
        } else {
            (s, "*".to_owned())
        };

        let parts: Vec<&str> = host_path.splitn(3, '/').collect();
        if parts.len() != 3 {
            return Err(format!(
                "invalid dependency origin '{s}': expected `host/user/repo[@semver]`"
            ));
        }

        Ok(DepOrigin {
            host: parts[0].to_owned(),
            user: parts[1].to_owned(),
            repo: parts[2].to_owned(),
            req,
        })
    }

    pub fn tarball_url(&self, version: &str) -> String {
        format!(
            "https://{}/{}/{}/archive/refs/tags/v{}.tar.gz",
            self.host, self.user, self.repo, version
        )
    }

    pub fn tags_api_url(&self) -> String {
        if self.host == "github.com" {
            format!(
                "https://api.github.com/repos/{}/{}/tags",
                self.user, self.repo
            )
        } else {
            // Gitea / Forgejo compatible
            format!(
                "https://{}/api/v1/repos/{}/{}/tags",
                self.host, self.user, self.repo
            )
        }
    }

    /// Logical name used as directory name inside `.vn/packages/`
    pub fn local_name(&self) -> String {
        format!(
            "{}_{}_{}",
            self.host.replace('.', "_"),
            self.user,
            self.repo
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectManifest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    /// Map of alias → origin string (`github.com/user/repo@^1.2.3`)
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

impl ProjectManifest {
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        serde_json::from_str(&raw).map_err(|e| format!("invalid {}: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize manifest: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    pub fn parsed_deps(&self) -> Result<HashMap<String, DepOrigin>, String> {
        self.dependencies
            .iter()
            .map(|(alias, origin_str)| {
                DepOrigin::parse(origin_str).map(|origin| (alias.clone(), origin))
            })
            .collect()
    }
}

pub fn find_project_manifest(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        // Skip inside .vn/packages/
        let mut in_pkg = false;
        let mut prev = None::<&std::ffi::OsStr>;
        for comp in dir.components() {
            let cur = comp.as_os_str();
            if prev == Some(std::ffi::OsStr::new(varn_modules::ENV_DIR_NAME))
                && cur == std::ffi::OsStr::new(varn_modules::MODULES_DIR_NAME)
            {
                in_pkg = true;
                break;
            }
            prev = Some(cur);
        }
        if in_pkg {
            continue;
        }

        let candidate = dir.join(varn_modules::PACKAGE_MANIFEST_FILE);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}
