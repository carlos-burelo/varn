use crate::{cli::InfoArgs, error::CliError, pipeline};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

pub fn execute(args: InfoArgs) -> Result<(), CliError> {
    let source = std::fs::read_to_string(&args.file)
        .map_err(|e| CliError::fatal(format!("no se puede leer '{}': {}", args.file, e)))?;

    let (tokens, lexeme_buf) = pipeline::lex_raw(&source, &args.file);
    let mut program = pipeline::parse_raw(tokens, lexeme_buf, &source, &args.file)?;
    varn_core::assign_ast_ids(&mut program);

    let (tree, stats) = build_info_tree(&args.file, &program)?;
    print_info(&args.file, &tree, &stats);
    Ok(())
}

struct Node {
    children: Vec<String>,
}

struct Stats {
    total_modules: usize,
    stdlib_count: usize,
    package_count: usize,
    local_count: usize,
}

fn build_info_tree(
    entry_path: &str,
    entry_program: &varn_core::ast::Program,
) -> Result<(HashMap<String, Node>, Stats), CliError> {
    let mut tree = HashMap::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    let mut stdlib_count = 0;
    let mut package_count = 0;
    let mut local_count = 0;

    let canonical_entry = canonical_or_original(Path::new(entry_path));
    queue.push_back((canonical_entry.clone(), Some(entry_program.clone())));

    while let Some((curr_path, program)) = queue.pop_front() {
        if visited.contains(&curr_path) {
            continue;
        }
        visited.insert(curr_path.clone());

        if varn_modules::is_embedded_module_path(&curr_path) {
            stdlib_count += 1;
        } else if varn_modules::is_package_module_path(&curr_path) {
            package_count += 1;
        } else {
            local_count += 1;
        }

        let prog = if let Some(p) = program {
            p
        } else {
            let source = std::fs::read_to_string(&curr_path)
                .map_err(|e| CliError::fatal(format!("cannot read module '{curr_path}': {e}")))?;
            let (tokens, lexeme_buf) = pipeline::lex_raw(&source, &curr_path);
            let mut p = pipeline::parse_raw(tokens, lexeme_buf, &source, &curr_path)?;
            varn_core::assign_ast_ids(&mut p);
            p
        };

        let mut children = Vec::new();
        let curr_dir = Path::new(&curr_path)
            .parent()
            .unwrap_or_else(|| Path::new("."));

        for import_spec in crate::import_collector::collect_imports(&prog) {
            if let Some(resolved) = crate::module_precompile::resolve_import_specifier(
                &import_spec,
                curr_dir,
                &mut HashMap::new(),
            )
            .map_err(CliError::fatal)?
            {
                children.push(resolved.clone());
                if !visited.contains(&resolved) {
                    queue.push_back((resolved, None));
                }
            }
        }

        tree.insert(curr_path.clone(), Node { children });
    }

    Ok((
        tree,
        Stats {
            total_modules: visited.len(),
            stdlib_count,
            package_count,
            local_count,
        },
    ))
}

fn print_info(entry: &str, tree: &HashMap<String, Node>, stats: &Stats) {
    use varn_debug::colors::{BOLD, R};

    println!();
    println!("  {BOLD}entrada:{R}   {entry}");
    println!("  {}módulos:{}   {}", BOLD, R, stats.total_modules);
    println!();
    println!("  {BOLD}grafo de dependencias:{R}");

    let canonical_entry = canonical_or_original(Path::new(entry));
    let mut visited = HashSet::new();
    print_tree_node(&canonical_entry, tree, &mut visited, "", true);

    println!();
    println!("  {}stdlib:{}    {} módulos", BOLD, R, stats.stdlib_count);
    println!("  {}packages:{}  {} módulos", BOLD, R, stats.package_count);
    println!("  {}locales:{}   {} módulos", BOLD, R, stats.local_count);
}

fn print_tree_node(
    path: &str,
    tree: &HashMap<String, Node>,
    visited: &mut HashSet<String>,
    prefix: &str,
    is_last: bool,
) {
    let node = match tree.get(path) {
        Some(n) => n,
        None => return,
    };

    let connector = if is_last { "└─ " } else { "├─ " };
    let display_path =
        if let Some(embedded) = path.strip_prefix(varn_modules::EMBEDDED_MODULE_PREFIX) {
            format!("std:{embedded} (builtin)")
        } else {
            path.to_owned()
        };

    println!("{prefix}{connector}{display_path}");

    if visited.contains(path) {
        return;
    }
    visited.insert(path.to_owned());

    let new_prefix = format!("{}{}", prefix, if is_last { "   " } else { "│  " });
    for (i, child) in node.children.iter().enumerate() {
        print_tree_node(
            child,
            tree,
            visited,
            &new_prefix,
            i == node.children.len() - 1,
        );
    }
}

fn canonical_or_original(path: &Path) -> String {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return normalize_path(canonical.to_string_lossy().into_owned());
    }
    normalize_path(path.to_string_lossy().into_owned())
}

fn normalize_path(path: String) -> String {
    #[cfg(windows)]
    {
        if let Some(rest) = path.strip_prefix("\\\\?\\") {
            return rest.to_owned();
        }
    }
    path
}
