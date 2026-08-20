use semver::{Version, VersionReq};

use crate::manifest::DepOrigin;

#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub version: String,
    pub commit: String,
}

pub fn resolve_version(origin: &DepOrigin) -> Result<ResolvedVersion, String> {
    match origin {
        DepOrigin::LocalPath { path } => {
            if !path.exists() {
                return Err(format!(
                    "local dependency path does not exist: {}",
                    path.display()
                ));
            }
            let version = if let Some(mpath) = crate::manifest::find_project_manifest(path) {
                if let Ok(manifest) = crate::manifest::ProjectManifest::load(&mpath) {
                    manifest.get_version().unwrap_or("0.0.0-local").to_owned()
                } else {
                    "0.0.0-local".to_owned()
                }
            } else {
                "0.0.0-local".to_owned()
            };
            Ok(ResolvedVersion {
                version,
                commit: "local".to_owned(),
            })
        }
        DepOrigin::Remote {
            host,
            user,
            repo,
            req: req_str,
        } => {
            let req = VersionReq::parse(req_str)
                .map_err(|e| format!("invalid semver constraint '{req_str}': {e}"))?;

            let tags = fetch_tags(origin)?;

            let mut candidates: Vec<(Version, String)> = tags
                .into_iter()
                .filter_map(|(tag, commit)| {
                    let ver_str = tag.strip_prefix('v').unwrap_or(&tag);
                    Version::parse(ver_str).ok().map(|v| (v, commit))
                })
                .filter(|(v, _)| req.matches(v))
                .collect();

            if candidates.is_empty() {
                return Err(format!(
                    "no version of {host}/{user}/{repo} satisfies '{req_str}'"
                ));
            }

            candidates.sort_by(|a, b| b.0.cmp(&a.0));
            let (best_ver, commit) = candidates.remove(0);

            Ok(ResolvedVersion {
                version: best_ver.to_string(),
                commit,
            })
        }
    }
}

fn fetch_tags(origin: &DepOrigin) -> Result<Vec<(String, String)>, String> {
    let url = origin.tags_api_url();

    let response = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .get(&url)
        .set("User-Agent", "varn-pm/0.1")
        .set("Accept", "application/json")
        .call()
        .map_err(|e| format!("cannot fetch tags for {url}: {e}"))?;

    let json: serde_json::Value = response
        .into_json()
        .map_err(|e| format!("invalid tags response: {e}"))?;

    let tags = json
        .as_array()
        .ok_or_else(|| "tags API returned non-array".to_owned())?;

    let mut result = Vec::with_capacity(tags.len());
    for tag in tags {
        let name = tag["name"].as_str().unwrap_or("").to_owned();

        let commit = tag
            .get("commit")
            .and_then(|c| c.get("sha"))
            .and_then(|s| s.as_str())
            .or_else(|| tag.get("id").and_then(|s| s.as_str()))
            .unwrap_or("")
            .to_owned();
        if !name.is_empty() && !commit.is_empty() {
            result.push((name, commit));
        }
    }

    Ok(result)
}
