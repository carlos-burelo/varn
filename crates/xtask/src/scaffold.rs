use std::path::{Path, PathBuf};

fn pascal(name: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn valid_mod(name: &str) -> bool {
    if name.is_empty() || !name.as_bytes()[0].is_ascii_lowercase() {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn valid_class(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => (),
        None | Some(_) => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn fn_module_rs(mod_name: &str) -> String {
    let struct_name = format!("{}Runtime", pascal(mod_name));
    format!(
        "use varn_op_macros::varn_contract;\nuse varn_types::NativeCtx;\n\npub struct {struct_name};\n\nvarn_contract! {{\n    module: \"runtime:{mod_name}\",\n    contract: \"src/modules/runtime/{mod_name}/{mod_name}_runtime.vn\",\n    impl {struct_name} {{\n        fn ping(_ctx: &mut dyn NativeCtx) -> Result<String, String> {{\n            Ok(\"pong\".to_string())\n        }}\n    }}\n}}\n"
    )
}

fn class_block_rs(mod_name: &str, class: &str) -> String {
    format!(
        "\nvarn_contract! {{\n    module: \"runtime:{mod_name}\",\n    class: \"{class}\",\n    contract: \"src/modules/runtime/{mod_name}/{mod_name}_runtime.vn\",\n    impl {class}Impl {{\n        fn constructor(\n            _ctx: &mut dyn NativeCtx,\n            this: varn_types::VmValue,\n        ) -> varn_types::VmValue {{\n            this\n        }}\n    }}\n}}\n"
    )
}

fn contract_vn(mod_name: &str, classes: &[String]) -> String {
    let mut out = "export declare function ping$(): str;\n".to_string();
    for class in classes {
        out.push_str(&format!(
            "\nexport declare class {class} {{\n    constructor();\n}}\n"
        ));
    }
    let _ = mod_name;
    out
}

fn std_mod_vn(mod_name: &str, classes: &[String]) -> String {
    let mut out = format!(
        "import {{ ping$ }} from \"runtime:{mod_name}\";\n\nexport function ping(): str {{\n    return ping$();\n}}\n"
    );
    for class in classes {
        out.push_str(&format!(
            "\nexport {{ {class} }} from \"runtime:{mod_name}\";\n"
        ));
    }
    out
}

fn marker_for_module(mod_name: &str) -> String {
    format!(
        "__VARN_LINK_MARKER_RUNTIME_{}",
        mod_name.to_ascii_uppercase()
    )
}

fn marker_for_class(class: &str) -> String {
    let mut out = String::from("__VARN_LINK_MARKER_");
    for c in class.chars() {
        if c.is_ascii_alphanumeric() {
            out.extend(c.to_uppercase());
        } else {
            out.push('_');
        }
    }
    out
}

fn insert_mod_pair(lines: &mut Vec<String>, mod_name: &str) -> bool {
    let pub_line = format!("pub mod {mod_name};");
    let path_line = format!("#[path = \"runtime/{mod_name}/{mod_name}.rs\"]");
    let mut pos = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("pub mod ") {
            let name = rest.trim_end_matches(';');
            if name > mod_name {
                pos = Some(i);
                break;
            }
        }
    }
    let mut idx = match pos {
        Some(i) => i,
        None => return false,
    };
    while idx > 0 && lines[idx - 1].trim().starts_with("#[path") {
        idx -= 1;
    }
    lines.insert(idx, path_line);
    lines.insert(idx + 1, pub_line);
    true
}

fn insert_sorted_line(
    lines: &mut Vec<String>,
    anchor_prefix: &str,
    new_line: String,
    key: &str,
) -> bool {
    let mut pos = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with(anchor_prefix) {
            continue;
        }
        let rest = trimmed[anchor_prefix.len()..].trim();
        if rest > key {
            pos = Some(i);
            break;
        }
    }
    match pos {
        Some(i) => {
            lines.insert(i, new_line);
            true
        }
        None => false,
    }
}

fn apply_registry(
    root: &Path,
    mod_name: &str,
    classes: &[String],
) -> Result<(String, String), String> {
    let path = root.join("crates/varn-builtins/src/modules/mod.rs");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();

    let pub_line = format!("pub mod {mod_name};");
    let mod_line = format!("#[path = \"runtime/{mod_name}/{mod_name}.rs\"]");
    if text.contains(&mod_line) || text.contains(&pub_line) {
        return Err(format!("module '{mod_name}' already registered"));
    }
    if !insert_mod_pair(&mut lines, mod_name) {
        return Err("no pub mod anchor found".to_string());
    }

    let mut markers = vec![marker_for_module(mod_name)];
    for class in classes {
        markers.push(marker_for_class(class));
    }
    for marker in &markers {
        let line = format!("    register_marker!({mod_name}, {marker});");
        if !insert_sorted_line(&mut lines, "register_marker!(", line, mod_name) {
            return Err("no register_marker anchor found".to_string());
        }
    }

    Ok((text, lines.join("\n") + "\n"))
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut mod_name = None;
    let mut classes: Vec<String> = Vec::new();
    let mut dry_run = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--class" => {
                i += 1;
                let c = args.get(i).ok_or("--class requires a Name")?.clone();
                if !valid_class(&c) {
                    return Err(format!("invalid class name '{c}': use PascalCase"));
                }
                classes.push(c);
            }
            "--dry-run" => dry_run = true,
            other if other.starts_with('-') => return Err(format!("unknown flag '{other}'")),
            other => {
                if mod_name.is_some() {
                    return Err("only one module name allowed".to_string());
                }
                mod_name = Some(other.to_string());
            }
        }
        i += 1;
    }
    let mod_name =
        mod_name.ok_or("usage: cargo xtask std-scaffold <mod> [--class Name] [--dry-run]")?;
    if !valid_mod(&mod_name) {
        return Err(format!(
            "invalid module name '{mod_name}': lowercase alphanumeric/underscore"
        ));
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .ok_or("cannot determine workspace root")?
        .to_path_buf();

    let rs_dir = root.join(format!(
        "crates/varn-builtins/src/modules/runtime/{mod_name}"
    ));
    let rs_file = rs_dir.join(format!("{mod_name}.rs"));
    let vn_file = rs_dir.join(format!("{mod_name}_runtime.vn"));
    let std_file = root.join(format!("std/{mod_name}/mod.vn"));

    let mut rs = fn_module_rs(&mod_name);
    for class in &classes {
        rs.push_str(&format!("pub struct {class}Impl;\n"));
        rs.push_str(&class_block_rs(&mod_name, class));
    }
    let contract = contract_vn(&mod_name, &classes);
    let std_mod = std_mod_vn(&mod_name, &classes);
    let (old_registry, new_registry) = apply_registry(&root, &mod_name, &classes)?;

    if dry_run {
        println!("--- {rs_file:?} ---\n{rs}");
        println!("--- {vn_file:?} ---\n{contract}");
        println!("--- {std_file:?} ---\n{std_mod}");
        println!("--- modules/mod.rs (××× old / +++ new) ---");
        for line in new_registry.lines() {
            if !old_registry.lines().any(|o| o == line) {
                println!("+++ {line}");
            }
        }
        return Ok(());
    }

    if rs_file.exists() || std_file.exists() {
        return Err(format!("module '{mod_name}' files already exist"));
    }
    std::fs::create_dir_all(&rs_dir).map_err(|e| format!("mkdir: {e}"))?;
    std::fs::create_dir_all(std_file.parent().unwrap()).map_err(|e| format!("mkdir: {e}"))?;
    std::fs::write(&rs_file, rs).map_err(|e| format!("write {}: {e}", rs_file.display()))?;
    std::fs::write(&vn_file, contract).map_err(|e| format!("write {}: {e}", vn_file.display()))?;
    std::fs::write(&std_file, std_mod).map_err(|e| format!("write {}: {e}", std_file.display()))?;
    std::fs::write(
        root.join("crates/varn-builtins/src/modules/mod.rs"),
        new_registry,
    )
    .map_err(|e| format!("write registry: {e}"))?;

    println!("scaffolded std:{mod_name} (runtime:{mod_name})");
    println!("next: cargo check -p varn-builtins, then extend the contract");
    Ok(())
}
