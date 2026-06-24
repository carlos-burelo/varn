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
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
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

                let vn_source_rel = vn_rel.trim_start_matches("src/").to_string();
                let vn_source_field = format!("crates/varn-builtins/src/{vn_source_rel}");

                let source_content = fs::read_to_string(&vn_rel)
                    .unwrap_or_else(|e| panic!("cannot read .vn source at {vn_rel}: {e}"));
                let exports = extract_exports_from_source(&source_content);

                emit_entries_from_json(&raw, &vn_source_field, &vn_rel, &exports, out);
            }

            collect_registry_entries(&path, out);
        }
    }
}

fn find_vn_source(dir: &Path) -> Option<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return None;
    };
    let mut candidates: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("vn")
        })
        .collect();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

fn emit_entries_from_json(
    json: &str,
    vn_source_field: &str,
    vn_include_path: &str,
    exports: &[String],
    out: &mut impl Write,
) {
    let json = json.trim();

    if json.contains("\"modules\"") {
        let inner = extract_array(json, "modules");
        for entry in parse_object_array(&inner) {
            if let (Some(id), Some(kind)) = (extract_str(&entry, "id"), extract_str(&entry, "kind"))
            {
                let capabilities = extract_capabilities(&entry);
                let pure_module = extract_bool(&entry, "pure");
                emit_spec_entry(
                    out,
                    &id,
                    &kind,
                    vn_source_field,
                    vn_include_path,
                    exports,
                    &capabilities,
                    pure_module,
                );
            }
        }
    } else {
        if let (Some(id), Some(kind)) = (extract_str(json, "id"), extract_str(json, "kind")) {
            let capabilities = extract_capabilities(json);
            let pure_module = extract_bool(json, "pure");
            emit_spec_entry(
                out,
                &id,
                &kind,
                vn_source_field,
                vn_include_path,
                exports,
                &capabilities,
                pure_module,
            );
        }
    }
}

fn emit_spec_entry(
    out: &mut impl Write,
    id: &str,
    kind: &str,
    vn_source_field: &str,
    vn_include_path: &str,
    exports: &[String],
    _capabilities: &[String],
    pure_module: bool,
) {
    let kind_expr = match kind {
        "core" => "ModuleKind::Core",
        "stdlib" => "ModuleKind::Stdlib",
        "runtime" => "ModuleKind::Runtime",
        other => panic!("unknown module kind: {other}"),
    };
    let mut exports_formatted = String::new();
    if !exports.is_empty() {
        exports_formatted.push_str("&[");
        for exp in exports {
            exports_formatted.push_str(&format!(r#""{exp}","#));
        }
        exports_formatted.push(']');
    } else {
        exports_formatted.push_str("&[]");
    }
    let pure_suffix = if pure_module { ".pure_module()" } else { "" };
    writeln!(
        out,
        r#"    ModuleSpec::new("{id}", {kind_expr}, "{vn_source_field}").with_source(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/{vn_include_path}"))).with_exports({exports_formatted}){pure_suffix},"#,
    ).unwrap();
}

fn extract_bool(json: &str, key: &str) -> bool {
    let needle = format!("\"{key}\"");
    let Some(pos) = json.find(&needle) else {
        return false;
    };
    let after_key = &json[pos + needle.len()..];
    let Some(colon) = after_key.find(':') else {
        return false;
    };
    let after_colon = after_key[colon + 1..].trim_start();
    after_colon.starts_with("true")
}

fn extract_str(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let pos = json.find(&needle)?;
    let after_key = &json[pos + needle.len()..];
    let colon = after_key.find(':')?;
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
                if depth == 0 {
                    start = Some(i);
                }
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

fn extract_exports_from_source(source: &str) -> Vec<String> {
    let mut exports = Vec::new();
    let mut brace_depth = 0;
    let mut chars = source.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '/' => {
                if chars.peek() == Some(&'/') {
                    chars.next();
                    while let Some(next_c) = chars.next() {
                        if next_c == '\n' {
                            break;
                        }
                    }
                } else if chars.peek() == Some(&'*') {
                    chars.next();
                    while let Some(next_c) = chars.next() {
                        if next_c == '*' && chars.peek() == Some(&'/') {
                            chars.next();
                            break;
                        }
                    }
                }
            }
            '"' | '\'' | '`' => {
                let quote = c;
                let mut escaped = false;
                while let Some(next_c) = chars.next() {
                    if escaped {
                        escaped = false;
                    } else if next_c == '\\' {
                        escaped = true;
                    } else if next_c == quote {
                        break;
                    }
                }
            }
            '{' => {
                brace_depth += 1;
            }
            '}' => {
                if brace_depth > 0 {
                    brace_depth -= 1;
                }
            }
            _ => {
                if brace_depth == 0 && (c.is_alphabetic() || c == '_') {
                    let mut word = String::new();
                    word.push(c);
                    while let Some(&next_c) = chars.peek() {
                        if next_c.is_alphanumeric() || next_c == '_' {
                            word.push(next_c);
                            chars.next();
                        } else {
                            break;
                        }
                    }

                    if word == "export" {
                        let mut words = Vec::new();
                        while words.len() < 5 {
                            skip_whitespace_and_comments(&mut chars);

                            if let Some(&next_c) = chars.peek() {
                                if next_c.is_alphanumeric() || next_c == '_' {
                                    let mut w = String::new();
                                    while let Some(&nc) = chars.peek() {
                                        if nc.is_alphanumeric() || nc == '_' {
                                            w.push(nc);
                                            chars.next();
                                        } else {
                                            break;
                                        }
                                    }
                                    words.push(w);
                                } else if next_c == '{'
                                    || next_c == '*'
                                    || next_c == '='
                                    || next_c == ';'
                                {
                                    break;
                                } else {
                                    chars.next();
                                    break;
                                }
                            } else {
                                break;
                            }
                        }

                        let mut keyword_idx = 0;
                        if keyword_idx < words.len() && words[keyword_idx] == "declare" {
                            keyword_idx += 1;
                        }

                        if keyword_idx < words.len() {
                            let kw = &words[keyword_idx];
                            if [
                                "function",
                                "class",
                                "interface",
                                "namespace",
                                "type",
                                "enum",
                                "struct",
                                "const",
                                "let",
                            ]
                            .contains(&kw.as_str())
                            {
                                if keyword_idx + 1 < words.len() {
                                    let name = &words[keyword_idx + 1];
                                    exports.push(name.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    exports.sort();
    exports.dedup();
    exports
}

fn skip_whitespace_and_comments(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '/' {
            chars.next();
            if chars.peek() == Some(&'/') {
                chars.next();
                while let Some(next_c) = chars.next() {
                    if next_c == '\n' {
                        break;
                    }
                }
            } else if chars.peek() == Some(&'*') {
                chars.next();
                while let Some(next_c) = chars.next() {
                    if next_c == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        break;
                    }
                }
            } else {
                break;
            }
        } else {
            break;
        }
    }
}
