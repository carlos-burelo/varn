use crate::{cli::InitArgs, error::CliError};
use std::path::Path;

pub fn execute(args: InitArgs) -> Result<(), CliError> {
    let dir = args.dir.as_deref().unwrap_or(".");
    let name = args.name.unwrap_or_else(|| {
        Path::new(dir)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("mi-proyecto")
            .to_owned()
    });

    if dir != "." {
        std::fs::create_dir_all(dir)
            .map_err(|e| CliError::fatal(format!("no se puede crear '{dir}': {e}")))?;
    }

    let main_path = if dir == "." {
        "main.vn".to_owned()
    } else {
        format!("{dir}/main.vn")
    };

    let manifest_path = if dir == "." {
        "varn.json".to_owned()
    } else {
        format!("{dir}/varn.json")
    };

    let wr_dir = if dir == "." {
        ".vn".to_owned()
    } else {
        format!("{dir}/.vn")
    };

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

    let env_path = format!("{wr_dir}/.env");
    if !Path::new(&env_path).exists() {
        std::fs::write(
            &env_path,
            "# Variables de entorno del proyecto\n# Este archivo NO debe commitearse\n",
        )
        .map_err(|e| CliError::fatal(format!("no se puede escribir '{env_path}': {e}")))?;
    }

    let gitignore_path = format!("{wr_dir}/.gitignore");
    std::fs::write(&gitignore_path, ".env\npackages/\ncache/\n")
        .map_err(|e| CliError::fatal(format!("no se puede escribir '{gitignore_path}': {e}")))?;

    println!("Proyecto inicializado en '{dir}'");
    println!("  {manifest_path}");
    println!("  {main_path}");
    println!("  {wr_dir}/.env");
    println!();
    println!("Ejecutar con:  wr run {main_path}");
    Ok(())
}
