use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DepOrigin {
    Remote {
        host: String,
        user: String,
        repo: String,
        req: String,
    },
    LocalPath {
        path: PathBuf,
    },
}

impl DepOrigin {
    pub fn parse(s: &str) -> Result<Self, String> {
        if let Some(path_str) = s.strip_prefix("path:") {
            return Ok(DepOrigin::LocalPath {
                path: PathBuf::from(path_str),
            });
        }

        let (host_path, req) = if let Some(at) = s.rfind('@') {
            (&s[..at], s[at + 1..].to_owned())
        } else {
            (s, "*".to_owned())
        };

        let parts: Vec<&str> = host_path.splitn(3, '/').collect();
        if parts.len() != 3 {
            return Err(format!(
                "invalid dependency origin '{s}': expected `host/user/repo[@semver]` or `path:<path>`"
            ));
        }

        Ok(DepOrigin::Remote {
            host: parts[0].to_owned(),
            user: parts[1].to_owned(),
            repo: parts[2].to_owned(),
            req,
        })
    }

    pub fn to_origin_string(&self) -> String {
        match self {
            DepOrigin::Remote {
                host,
                user,
                repo,
                req,
            } => {
                if req == "*" {
                    format!("{host}/{user}/{repo}")
                } else {
                    format!("{host}/{user}/{repo}@{req}")
                }
            }
            DepOrigin::LocalPath { path } => {
                format!("path:{}", path.display())
            }
        }
    }

    pub fn tarball_url(&self, version: &str) -> String {
        match self {
            DepOrigin::Remote {
                host, user, repo, ..
            } => varn_modules::resolver::forge_tarball_url(host, user, repo, version),
            DepOrigin::LocalPath { path } => format!("file://{}", path.display()),
        }
    }

    pub fn tags_api_url(&self) -> String {
        match self {
            DepOrigin::Remote {
                host, user, repo, ..
            } => varn_modules::resolver::forge_tags_api_url(host, user, repo),
            DepOrigin::LocalPath { path } => format!("file://{}", path.display()),
        }
    }

    pub fn local_name(&self) -> String {
        match self {
            DepOrigin::Remote {
                host, user, repo, ..
            } => {
                format!("{}_{}_{}", host.replace('.', "_"), user, repo)
            }
            DepOrigin::LocalPath { path } => {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("local_pkg");
                format!("local_{name}")
            }
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PackageSection {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub main: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProjectManifest {
    #[serde(default)]
    pub package: Option<PackageSection>,

    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub main: Option<String>,

    #[serde(default)]
    pub exports: HashMap<String, String>,

    #[serde(default)]
    pub dependencies: HashMap<String, String>,

    #[serde(default, alias = "dev-dependencies", alias = "dev_dependencies")]
    pub dev_dependencies: HashMap<String, String>,

    #[serde(default, alias = "peer-dependencies", alias = "peer_dependencies")]
    pub peer_dependencies: HashMap<String, String>,

    #[serde(default)]
    pub workspaces: Vec<String>,
}

impl ProjectManifest {
    pub fn get_name(&self) -> Option<&str> {
        self.package.as_ref().and_then(|p| p.name.as_deref()).or(self.name.as_deref())
    }

    pub fn get_version(&self) -> Option<&str> {
        self.package.as_ref().and_then(|p| p.version.as_deref()).or(self.version.as_deref())
    }

    pub fn get_main(&self) -> Option<&str> {
        self.package.as_ref().and_then(|p| p.main.as_deref()).or(self.main.as_deref())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        toml::from_str(&raw).map_err(|e| format!("invalid {}: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize manifest: {e}"))?;
        std::fs::write(path, content).map_err(|e| format!("cannot write {}: {e}", path.display()))
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

        let toml_candidate = dir.join(varn_modules::PACKAGE_MANIFEST_FILE);
        if toml_candidate.exists() {
            return Some(toml_candidate);
        }
        let vn_toml = dir.join(varn_modules::PACKAGE_MANIFEST_FILE_VN);
        if vn_toml.exists() {
            return Some(vn_toml);
        }
    }
    None
}
