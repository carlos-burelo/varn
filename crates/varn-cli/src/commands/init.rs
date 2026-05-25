use crate::{cli::InitArgs, error::CliError};
use std::path::Path;

pub fn execute(args: InitArgs) -> Result<(), CliError> {
    let dir = args.dir.as_deref().unwrap_or(".");
    let base = Path::new(dir);
    let name = args.name.unwrap_or_else(|| {
        base.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("mi-proyecto")
            .to_owned()
    });

    if dir != "." {
        std::fs::create_dir_all(dir)
            .map_err(|e| CliError::fatal(format!("no se puede crear '{dir}': {e}")))?;
    }

    let main_path = base.join("main.vn").to_string_lossy().to_string();
    let manifest_path = base.join("varn.json").to_string_lossy().to_string();
    let wr_dir = base.join(".vn").to_string_lossy().to_string();

    if Path::new(&main_path).exists() {
        return Err(CliError::fatal(format!("'{main_path}' ya existe")));
    }
    if Path::new(&manifest_path).exists() {
        return Err(CliError::fatal(format!("'{manifest_path}' ya existe")));
    }

    let main_content = format!("// {name}\n\nprint(\"Hola desde {name}!\")\n");
    std::fs::write(&main_path, main_content)
        .map_err(|e| CliError::fatal(format!("no se puede escribir '{main_path}': {e}")))?;

    let manifest_content = format!(
        r#"{{
  "project": {{
    "name": "{name}",
    "version": "0.1.0"
  }},
  "bin": {{
    "main": "main.vn"
  }},
  "dependencies": {{}}
}}
"#
    );
    std::fs::write(&manifest_path, manifest_content)
        .map_err(|e| CliError::fatal(format!("no se puede escribir '{manifest_path}': {e}")))?;

    std::fs::create_dir_all(&wr_dir)
        .map_err(|e| CliError::fatal(format!("no se puede crear '{wr_dir}': {e}")))?;

    let wr_base = Path::new(&wr_dir);
    let env_path = wr_base.join(".env").to_string_lossy().to_string();
    if !Path::new(&env_path).exists() {
        std::fs::write(
            &env_path,
            "# Variables de entorno del proyecto\n# Este archivo NO debe commitearse\n",
        )
        .map_err(|e| CliError::fatal(format!("no se puede escribir '{env_path}': {e}")))?;
    }

    let gitignore_path = wr_base.join(".gitignore").to_string_lossy().to_string();
    std::fs::write(&gitignore_path, ".env\npackages/\ncache/\n")
        .map_err(|e| CliError::fatal(format!("no se puede escribir '{gitignore_path}': {e}")))?;

    println!("Proyecto inicializado en '{dir}'");
    println!("  {manifest_path}");
    println!("  {main_path}");
    println!("  {env_path}");
    println!();
    println!("Ejecutar con:  vn run {main_path}");
    Ok(())
}
