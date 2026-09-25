use std::fs;
use std::io::Write;
use std::path::Path;

/// A module's id is its place in the tree (ADR-0018): the directory
/// `src/modules/<layer>/<path>/` holding one `.vn` contract is `<layer>:<path>`.
/// No manifest restates it.
fn main() {
    println!("cargo:rerun-if-changed=src/modules");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir).join("registry.generated.rs");
    let mut out = fs::File::create(&out_path).expect("failed to create registry.generated.rs");

    writeln!(out, "pub static MODULE_REGISTRY: &[ModuleSpec] = &[").unwrap();
    for (layer, kind) in [
        ("core", "ModuleKind::Core"),
        ("runtime", "ModuleKind::Runtime"),
    ] {
        let root = Path::new("src/modules").join(layer);
        collect_modules(&root, &root, layer, kind, &mut out);
    }
    writeln!(out, "];").unwrap();
}

fn collect_modules(root: &Path, dir: &Path, layer: &str, kind: &str, out: &mut impl Write) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();

    let contracts: Vec<&Path> = paths
        .iter()
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("vn"))
        .map(|p| p.as_path())
        .collect();
    match contracts.as_slice() {
        [] => {}
        [contract] => {
            let rel = dir
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            assert!(
                !rel.is_empty(),
                "{} is a layer root, not a module",
                dir.display()
            );
            emit_spec_entry(out, &format!("{layer}:{rel}"), kind, contract);
        }
        many => panic!(
            "{} holds {} .vn contracts; a module is exactly one",
            dir.display(),
            many.len()
        ),
    }

    for sub in paths.iter().filter(|p| p.is_dir()) {
        collect_modules(root, sub, layer, kind, out);
    }
}

fn emit_spec_entry(out: &mut impl Write, id: &str, kind_expr: &str, contract: &Path) {
    let include_path = contract.to_string_lossy().replace('\\', "/");
    let source =
        fs::read_to_string(contract).unwrap_or_else(|e| panic!("cannot read {include_path}: {e}"));
    let exports: String = extract_exports_from_source(&source)
        .iter()
        .map(|e| format!(r#""{e}","#))
        .collect();
    let code = if has_code(&source) {
        ".with_code()"
    } else {
        ""
    };
    writeln!(
        out,
        r#"    ModuleSpec::new("{id}", {kind_expr}, "crates/varn-builtins/{include_path}").with_source(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/{include_path}"))).with_exports(&[{exports}]){code},"#,
    )
    .unwrap();
}

/// A contract exporting anything but native declarations, types and
/// interfaces carries Varn code that must be compiled and run.
fn has_code(source: &str) -> bool {
    source.lines().any(|line| {
        line.strip_prefix("export ").is_some_and(|rest| {
            !["declare ", "type ", "interface "]
                .iter()
                .any(|k| rest.trim_start().starts_with(k))
        })
    })
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
                    for next_c in chars.by_ref() {
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
                for next_c in chars.by_ref() {
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
                if brace_depth == 0 && (c.is_alphabetic() || c == '_' || c == '$') {
                    let mut word = String::new();
                    word.push(c);
                    while let Some(&next_c) = chars.peek() {
                        if next_c.is_alphanumeric() || next_c == '_' || next_c == '$' {
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
                                if next_c.is_alphanumeric() || next_c == '_' || next_c == '$' {
                                    let mut w = String::new();
                                    while let Some(&nc) = chars.peek() {
                                        if nc.is_alphanumeric() || nc == '_' || nc == '$' {
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
                                && keyword_idx + 1 < words.len()
                            {
                                let name = &words[keyword_idx + 1];
                                exports.push(name.clone());
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
                for next_c in chars.by_ref() {
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
