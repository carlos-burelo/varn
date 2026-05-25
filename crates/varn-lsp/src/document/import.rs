use std::collections::HashSet;

use super::ImportPathContext;

pub use varn_modules::resolver::{
    normalize_display_path, path_to_uri, percent_decode, relative_import_path, uri_to_path,
};

fn is_import_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("import") || t.starts_with("export") || line.contains(" from ")
}

pub fn import_path_at(source: &str, line: u32, col: u32) -> Option<ImportPathContext> {
    let src_line = source.lines().nth(line as usize)?;

    if !is_import_line(src_line) {
        return None;
    }

    let mut quote_char = None;
    let mut start_idx = None;
    let mut end_idx = None;

    for (i, c) in src_line.chars().enumerate() {
        if c == '\'' || c == '"' {
            if let Some(q) = quote_char {
                if q == c {
                    end_idx = Some(i);
                    if i >= col as usize && start_idx.unwrap_or(0) <= col as usize {
                        break;
                    }
                    quote_char = None;
                    start_idx = None;
                    end_idx = None;
                }
            } else {
                quote_char = Some(c);
                start_idx = Some(i);
            }
        }
    }

    if let (Some(start), Some(end)) = (start_idx, end_idx) {
        if col as usize > start && col as usize <= end {
            let full_specifier = &src_line[start + 1..end];
            let prefix = &src_line[start + 1..col as usize];
            return Some(ImportPathContext {
                prefix: prefix.to_owned(),
                specifier: full_specifier.to_owned(),
                content_start_col: (start + 1) as u32,
            });
        }
    }
    None
}

pub fn named_import_module_at(source: &str, line: u32, col: u32) -> Option<String> {
    let src_line = source.lines().nth(line as usize)?;
    if col as usize > src_line.len() {
        return None;
    }
    let before = &src_line[..col as usize];
    if !before.contains('{') {
        return None;
    }
    let after = &src_line[col as usize..];
    let full = if let Some(close_idx) = after.find('}') {
        format!("{}{}", before, &after[..=close_idx])
    } else {
        return None;
    };

    if !full.contains("import") && !full.contains("export") {
        return None;
    }

    let from_part = source.lines().skip(line as usize).find_map(|l| {
        if let Some(idx) = l.find("from") {
            let rest = &l[idx + 4..].trim();
            if (rest.starts_with('\'') && rest.ends_with('\''))
                || (rest.starts_with('"') && rest.ends_with('"'))
            {
                return Some(rest[1..rest.len() - 1].to_owned());
            }
        }
        None
    });

    from_part
}

pub fn named_imported_names_at(source: &str, line: u32, _col: u32) -> HashSet<String> {
    let mut names = HashSet::new();
    let src_line = match source.lines().nth(line as usize) {
        Some(l) => l,
        None => return names,
    };
    let open_idx = match src_line.find('{') {
        Some(i) => i,
        None => return names,
    };
    let close_idx = match src_line.find('}') {
        Some(i) => i,
        None => return names,
    };
    let inner = &src_line[open_idx + 1..close_idx];
    for part in inner.split(',') {
        let name = part.trim();
        if !name.is_empty() && name != "import" && name != "export" {
            names.insert(name.to_owned());
        }
    }
    names
}
