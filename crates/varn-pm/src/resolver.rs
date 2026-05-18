use semver::{Version, VersionReq};

use crate::manifest::DepOrigin;

#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub version: String,
    pub commit: String,
}

pub fn resolve_version(origin: &DepOrigin) -> Result<ResolvedVersion, String> {
    let req = VersionReq::parse(&origin.req)
        .map_err(|e| format!("invalid semver constraint '{}': {e}", origin.req))?;

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
            "no version of {}/{}/{} satisfies '{}'",
            origin.host, origin.user, origin.repo, origin.req
        ));
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    let (best_ver, commit) = candidates.remove(0);

    Ok(ResolvedVersion {
        version: best_ver.to_string(),
        commit,
    })
}

fn fetch_tags(origin: &DepOrigin) -> Result<Vec<(String, String)>, String> {
    let url = origin.tags_api_url();

    let response = ureq::get(&url)
        .set("User-Agent", "varn-pm/0.1")
        .set("Accept", "application/json")
        .call()
        .map_err(|e| {
            format!(
                "cannot fetch tags for {}/{}/{}: {e}",
                origin.host, origin.user, origin.repo
            )
        })?;

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
