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

fn module_rs(mod_name: &str, classes: &[String]) -> String {
    let struct_name = format!("{}Runtime", pascal(mod_name));
    let mut out = format!(
        "use varn_macros::varn_contract;\nuse varn_types::NativeCtx;\n\npub struct {struct_name};\n"
    );
    for class in classes {
        out.push_str(&format!("pub struct {class}Impl;\n"));
    }
    out.push_str(&format!(
        "\nvarn_contract! {{\n    module: \"runtime:{mod_name}\",\n    contract: \"src/modules/runtime/{mod_name}/{mod_name}_runtime.vn\",\n    impl {struct_name} {{\n        fn ping(_ctx: &mut dyn NativeCtx) -> Result<String, String> {{\n            Ok(\"pong\".to_string())\n        }}\n    }}\n"
    ));
    for class in classes {
        out.push_str(&format!(
            "\n    class: \"{class}\",\n    impl {class}Impl {{\n        fn constructor(\n            _ctx: &mut dyn NativeCtx,\n            this: varn_types::VmValue,\n        ) -> varn_types::VmValue {{\n            this\n        }}\n    }}\n"
        ));
    }
    out.push_str("}\n");
    out
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

fn registry_entry(line: &str) -> Option<&str> {
    let name = line.strip_prefix("    ")?.split(" = \"").next()?;
    (line.contains(" = \"") && name.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        .then_some(name)
}

fn apply_registry(root: &Path, mod_name: &str) -> Result<(String, String), String> {
    let path = root.join("crates/varn-builtins/src/modules/mod.rs");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    if lines.iter().any(|l| registry_entry(l) == Some(mod_name)) {
        return Err(format!("module '{mod_name}' already registered"));
    }
    let pos = lines
        .iter()
        .position(|l| registry_entry(l).is_some_and(|name| name > mod_name))
        .or_else(|| {
            lines
                .iter()
                .rposition(|l| registry_entry(l).is_some())
                .map(|i| i + 1)
        })
        .ok_or("no native_modules! entries found")?;
    lines.insert(
        pos,
        format!("    {mod_name} = \"runtime/{mod_name}/{mod_name}.rs\" => [{mod_name}::NATIVE_ENTRIES];"),
    );
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

    let rs = module_rs(&mod_name, &classes);
    let contract = contract_vn(&mod_name, &classes);
    let std_mod = std_mod_vn(&mod_name, &classes);
    let (old_registry, new_registry) = apply_registry(&root, &mod_name)?;

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
