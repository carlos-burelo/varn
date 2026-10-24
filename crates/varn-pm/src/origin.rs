use std::collections::BTreeMap;
use std::path::PathBuf;

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

pub fn parse_dependencies(
    dependencies: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, DepOrigin>, String> {
    dependencies
        .iter()
        .map(|(alias, origin_str)| {
            DepOrigin::parse(origin_str).map(|origin| (alias.clone(), origin))
        })
        .collect()
}
