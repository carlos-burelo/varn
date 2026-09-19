//! Carga `.env` / `.env.local` del proyecto antes de que arranque nada más.
//!
//! El caso que motiva esto: depurar un panic de Rust (un `unwrap`/`expect`
//! reventando dentro del VM) exige `RUST_BACKTRACE=1`, y las variables de
//! depuración propias de Varn (`VARN_NO_JIT`, `VARN_GC_TRACE`, `VARN_DEBUG_OPS`,
//! …) se exportan a mano en cada shell nueva y se olvidan. Un `.env` en la
//! raíz del proyecto las fija una sola vez, versionado o no según el equipo
//! decida.
//!
//! Reglas, deliberadamente simples:
//! - Una variable YA presente en el entorno del proceso SIEMPRE gana — `.env`
//!   solo rellena lo que falta, nunca pisa lo que el shell/CI ya exportó.
//! - `.env.local` gana sobre `.env` (mismo orden que Vite/Next.js): se lee
//!   primero, así sus claves quedan puestas antes de que `.env` intente
//!   rellenarlas y las encuentre ya presentes.
//! - Se busca en el directorio de trabajo actual y, si difiere, en la raíz
//!   del proyecto (el primer ancestro con manifiesto) — cubre tanto
//!   `vn run script.vn` desde la raíz como desde un subdirectorio.
//! - Formato mínimo: `CLAVE=valor` por línea, `#` inicial o línea en blanco
//!   se ignoran, `export ` al frente se tolera, comillas simples/dobles que
//!   envuelven el valor completo se despojan. Sin interpolación de
//!   variables ni continuación de línea: ese alcance no hace falta aquí.
use std::path::{Path, PathBuf};

/// Punto de entrada: se llama una vez, lo primero en `main`.
pub fn load() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = varn_modules::artifact::find_project_root(&cwd);

    let mut dirs: Vec<&Path> = vec![cwd.as_path()];
    if root != cwd {
        dirs.push(root.as_path());
    }

    // `.env.local` primero: sus claves quedan puestas antes de que `.env`
    // las encuentre ya presentes y las salte.
    for dir in &dirs {
        load_file(&dir.join(".env.local"));
    }
    for dir in &dirs {
        load_file(&dir.join(".env"));
    }
}

fn load_file(path: &Path) {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return;
    };
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        // Una variable que ya vive en el entorno del proceso (exportada por
        // el shell, o por un `.env.local`/`.env` de un directorio más
        // específico ya cargado) gana siempre.
        if std::env::var_os(key).is_some() {
            continue;
        }
        let value = unquote(value.trim());
        // SAFETY: llamado una única vez desde `main`, antes de que arranque
        // ningún hilo — no hay lectura concurrente de `std::env` con la que
        // esta escritura pueda competir.
        unsafe {
            std::env::set_var(key, value);
        }
    }
}

/// Despoja un par de comillas que envuelva el valor COMPLETO (`"x"`, `'x'`).
/// Comillas parciales o desbalanceadas se dejan tal cual — no es este
/// parser el que decide que la sintaxis del archivo está mal.
fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' || first == b'\'') && first == last {
            return &value[1..value.len() - 1];
        }
    }
    value
}
