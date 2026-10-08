use crate::cli::GenContractTablesArgs;
use crate::error::CliError;

pub fn execute(args: GenContractTablesArgs) -> Result<(), CliError> {
    run(&args.crate_dir, &args.out, args.check).map_err(CliError::fatal)
}

use super::contract_members::{parse_contract_file, TableMember};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn contract_refs(rs_source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = rs_source;
    while let Some(pos) = rest.find("contract:") {
        rest = &rest[pos + "contract:".len()..];
        let trimmed = rest.trim_start();
        if let Some(quoted) = trimmed.strip_prefix('"') {
            if let Some(end) = quoted.find('"') {
                let lit = &quoted[..end];
                if !lit.is_empty() && !lit.contains('\n') && !out.contains(&lit.to_string()) {
                    out.push(lit.to_string());
                }
                rest = &quoted[end.min(quoted.len())..];
            }
        }
    }
    out.sort();
    out
}

fn find_crate_dir_arg(arg: &str) -> Option<PathBuf> {
    if !arg.is_empty() {
        return Some(PathBuf::from(arg));
    }
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("crates/varn-builtins/Cargo.toml").is_file() {
            return Some(dir.join("crates/varn-builtins"));
        }
        if dir.join("Cargo.toml").is_file() && dir.file_name().is_some_and(|n| n == "varn-builtins")
        {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn file_hash(path: &Path) -> Result<String, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
    Ok(format!("{:016x}", xxhash_rust::xxh3::xxh3_64(&bytes)))
}

pub(crate) fn generate(crate_dir: &Path) -> Result<String, String> {
    let mut refs = Vec::new();
    let rs_files = rs_file_list(&crate_dir.join("src"));
    for rs in &rs_files {
        let text = std::fs::read_to_string(rs)
            .map_err(|e| format!("cannot read '{}': {e}", rs.display()))?;
        refs.extend(contract_refs(&text));
    }
    refs.sort();
    refs.dedup();
    if refs.is_empty() {
        return Err("no `contract:` references found".to_string());
    }
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut contracts = serde_json::Map::new();
    for r in &refs {
        let path = crate_dir.join(r);
        files.insert(r.clone(), file_hash(&path)?);
        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
        let parsed = parse_contract_file(&source, r)?;
        let mut entry = serde_json::Map::new();
        let mut classes = serde_json::Map::new();
        for (name, members) in &parsed.classes {
            classes.insert(name.clone(), json_members(members));
        }
        entry.insert("classes".to_string(), serde_json::Value::Object(classes));
        entry.insert("functions".to_string(), json_members(&parsed.functions));
        contracts.insert(r.clone(), serde_json::Value::Object(entry));
    }
    let mut root = serde_json::Map::new();
    root.insert(
        "version".to_string(),
        serde_json::Value::Number(serde_json::Number::from(1)),
    );
    root.insert(
        "files".to_string(),
        serde_json::Value::Object(
            files
                .into_iter()
                .map(|(k, v)| (k, serde_json::Value::String(v)))
                .collect(),
        ),
    );
    root.insert(
        "contracts".to_string(),
        serde_json::Value::Object(contracts),
    );
    serde_json::to_string_pretty(&serde_json::Value::Object(root))
        .map(|mut s| {
            s.push('\n');
            s
        })
        .map_err(|e| format!("cannot serialize contract tables: {e}"))
}

fn json_members(members: &[TableMember]) -> serde_json::Value {
    serde_json::Value::Array(
        members
            .iter()
            .map(|m| {
                serde_json::json!({
                    "symbol": m.symbol,
                    "kind": m.kind,
                    "params": m.params.iter().map(|(ty, rest)| serde_json::json!({"ty": ty, "rest": rest})).collect::<Vec<_>>(),
                    "ret": m.ret,
                    "fallible": m.fallible,
                })
            })
            .collect(),
    )
}

fn rs_file_list(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_rs_files(dir, &mut out);
    out.sort();
    out
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

pub(crate) fn run(crate_dir_arg: &str, out_arg: &str, check: bool) -> Result<(), String> {
    let crate_dir = find_crate_dir_arg(crate_dir_arg)
        .ok_or_else(|| "cannot locate varn-builtins crate; pass --crate-dir".to_string())?;
    let out = if out_arg.is_empty() {
        crate_dir.join("contracts.json")
    } else {
        PathBuf::from(out_arg)
    };
    let rendered = generate(&crate_dir)?;
    if check {
        let current = std::fs::read_to_string(&out)
            .map_err(|e| format!("cannot read '{}': {e}", out.display()))?;
        if current != rendered {
            return Err(format!(
                "contract tables stale: '{}' differs; run `vn gen-contract-tables`",
                out.display()
            ));
        }
        return Ok(());
    }
    std::fs::write(&out, rendered).map_err(|e| format!("cannot write '{}': {e}", out.display()))?;
    Ok(())
}
