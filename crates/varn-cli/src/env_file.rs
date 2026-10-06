





















use std::path::{Path, PathBuf};


pub fn load() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = varn_modules::artifact::find_project_root(&cwd);

    let mut dirs: Vec<&Path> = vec![cwd.as_path()];
    if root != cwd {
        dirs.push(root.as_path());
    }

    
    
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
        
        
        
        if std::env::var_os(key).is_some() {
            continue;
        }
        let value = unquote(value.trim());
        
        
        
        unsafe {
            std::env::set_var(key, value);
        }
    }
}




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
