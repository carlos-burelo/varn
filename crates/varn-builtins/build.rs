use std::fs;
use std::io::Write;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/modules");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir).join("registry.generated.rs");
    let mut out = fs::File::create(&out_path).expect("failed to create registry.generated.rs");

    writeln!(out, "pub static MODULE_REGISTRY: &[ModuleSpec] = &[").unwrap();

    let modules_dir = Path::new("src/modules");
    collect_registry_entries(modules_dir, &mut out);

    writeln!(out, "];").unwrap();
}

fn collect_registry_entries(dir: &Path, out: &mut impl Write) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();

    for path in paths {
        if path.is_dir() {
            let json_path = path.join("module.json");
            if json_path.exists() {
                let raw = fs::read_to_string(&json_path)
                    .unwrap_or_else(|e| panic!("cannot read {}: {e}", json_path.display()));

                let vn_rel = find_vn_source(&path)
                    .unwrap_or_else(|| panic!("cannot find .vn source in {}", path.display()));
                // Relative to crate src root (for the vn_source field).
                let vn_source_rel = vn_rel
                    .trim_start_matches("src/")
                    .to_string();
                let vn_source_field = format!("crates/varn-builtins/src/{vn_source_rel}");

                // Parse JSON — handle both {"id":...} and {"modules":[...]} forms.
                emit_entries_from_json(&raw, &vn_source_field, &vn_rel, out);
            }
            // Recurse into subdirectories.
            collect_registry_entries(&path, out);
        }
    }
}

fn find_vn_source(dir: &Path) -> Option<String> {
    let Ok(entries) = fs::read_dir(dir) else { return None };
    let mut candidates: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("vn"))
        .collect();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

fn emit_entries_from_json(json: &str, vn_source_field: &str, vn_include_path: &str, out: &mut impl Write) {
    // Minimal JSON parsing — avoid pulling in serde_json at build time.
    let json = json.trim();

    if json.contains("\"modules\"") {
        // Array form: {"modules": [...]}
        let inner = extract_array(json, "modules");
        for entry in parse_object_array(&inner) {
            if let (Some(id), Some(kind)) = (extract_str(&entry, "id"), extract_str(&entry, "kind")) {
                let capabilities = extract_capabilities(&entry);
                emit_spec_entry(out, &id, &kind, vn_source_field, vn_include_path, &capabilities);
            }
        }
    } else {
        // Single object form: {"id": "...", "kind": "..."}
        if let (Some(id), Some(kind)) = (extract_str(json, "id"), extract_str(json, "kind")) {
            let capabilities = extract_capabilities(json);
            emit_spec_entry(out, &id, &kind, vn_source_field, vn_include_path, &capabilities);
        }
    }
}

fn emit_spec_entry(
    out: &mut impl Write,
    id: &str,
    kind: &str,
    vn_source_field: &str,
    vn_include_path: &str,
    _capabilities: &[String],
) {
    let kind_expr = match kind {
        "core" => "ModuleKind::Core",
        "stdlib" => "ModuleKind::Stdlib",
        "runtime" => "ModuleKind::Runtime",
        other => panic!("unknown module kind: {other}"),
    };
    writeln!(
        out,
        r#"    ModuleSpec::new("{id}", {kind_expr}, "{vn_source_field}").with_source(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/{vn_include_path}"))),"#,
    ).unwrap();
}

fn extract_str(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let pos = json.find(&needle)?;
    let after_key = &json[pos + needle.len()..];
    let colon = after_key.find(':')? ;
    let after_colon = after_key[colon + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let inner = &after_colon[1..];
    let end = inner.find('"')?;
    Some(inner[..end].to_string())
}

fn extract_array(json: &str, key: &str) -> String {
    let needle = format!("\"{key}\"");
    let pos = json.find(&needle).unwrap_or(0);
    let after = &json[pos + needle.len()..];
    let bracket = after.find('[').unwrap_or(0);
    let after_bracket = &after[bracket + 1..];
    let mut depth = 1i32;
    let mut end = 0;
    for (i, c) in after_bracket.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    after_bracket[..end].to_string()
}

fn parse_object_array(inner: &str) -> Vec<String> {
    let mut objs = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    for (i, c) in inner.char_indices() {
        match c {
            '{' => {
                if depth == 0 { start = Some(i); }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        objs.push(inner[s..=i].to_string());
                        start = None;
                    }
                }
            }
            _ => {}
        }
    }
    objs
}

fn extract_capabilities(json: &str) -> Vec<String> {
    if !json.contains("\"capabilities\"") {
        return Vec::new();
    }
    let inner = extract_array(json, "capabilities");
    inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
