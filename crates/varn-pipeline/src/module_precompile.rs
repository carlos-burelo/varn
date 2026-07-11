use crate::hash::fnv1a64;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use varn_core::ast::Program;
use varn_opt::FunctionProto;
use varn_types::PackageNode;

const UNKNOWN_INTEGRITY_HASH: &str = "0000000000000000";

pub struct ModuleGraphBuild {
    pub entry_path: String,
    pub modules: HashMap<String, FunctionProto>,
    pub source_hashes: HashMap<String, u64>,
    pub package_nodes: Vec<varn_types::PackageNode>,

    pub deps: HashMap<String, Vec<String>>,
}

pub fn build_module_graph(
    entry_program: &Program,
    entry_source: &str,
    entry_path: &str,
    entry_proto: &FunctionProto,
) -> Result<ModuleGraphBuild, String> {
    let canonical_entry = varn_modules::canonical_or_original(Path::new(entry_path));

    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    let mut source_hashes: HashMap<String, u64> = HashMap::new();
    source_hashes.insert(canonical_entry.clone(), fnv1a64(entry_source.as_bytes()));

    let mut node_sources: HashMap<String, (String, Program)> = HashMap::new();
    let mut package_nodes: HashMap<String, PackageNode> = HashMap::new();

    let entry_dir = Path::new(&canonical_entry)
        .parent()
        .unwrap_or_else(|| Path::new("."));

    let mut entry_deps = Vec::new();
    for spec in crate::import_collector::collect_imports(entry_program) {
        if let Some(dep_path) = resolve_import_specifier(&spec, entry_dir, &mut package_nodes)
            .map_err(|e| format!("{e}\n  imported from: {entry_path}"))?
        {
            entry_deps.push(dep_path);
        }
    }
    graph.insert(canonical_entry.clone(), entry_deps.clone());

    let mut queue: VecDeque<String> = entry_deps.into_iter().collect();
    let mut enqueued: HashSet<String> = HashSet::new();
    enqueued.insert(canonical_entry.clone());

    while let Some(module_path) = queue.pop_front() {
        if enqueued.contains(&module_path) {
            continue;
        }
        enqueued.insert(module_path.clone());

        if let Some(provider) = varn_modules::provider::get() {
            if provider.bytecode_blob(&module_path).is_some() {
                graph.insert(module_path.clone(), Vec::new());
                continue;
            }
        }

        let source = read_module_source(&module_path)
            .map_err(|e| format!("cannot read module '{module_path}': {e}"))?;
        source_hashes.insert(module_path.clone(), fnv1a64(source.as_bytes()));

        let (tokens, lexeme_buf, _) = varn_lexer::scan(&source, &module_path);
        let mut program = varn_parser::parse(tokens, lexeme_buf, &module_path)
            .map_err(|errs| format!("parse error in '{}': {}", module_path, errs[0].message))?;
        varn_core::assign_ast_ids(&mut program);

        let module_dir = Path::new(&module_path)
            .parent()
            .unwrap_or_else(|| Path::new("."));

        let mut deps = Vec::new();
        for child_spec in crate::import_collector::collect_imports(&program) {
            if let Some(child_path) =
                resolve_import_specifier(&child_spec, module_dir, &mut package_nodes)
                    .map_err(|e| format!("{e}\n  imported from: {module_path}"))?
            {
                deps.push(child_path.clone());
                if !enqueued.contains(&child_path) {
                    queue.push_back(child_path);
                }
            }
        }

        graph.insert(module_path.clone(), deps);
        node_sources.insert(module_path, (source, program));
    }

    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for (node, deps) in &graph {
        in_degree.entry(node.as_str()).or_insert(0);
        for dep in deps {
            if graph.contains_key(dep.as_str()) {
                *in_degree.entry(dep.as_str()).or_insert(0) += 1;
            }
        }
    }

    let mut topo_queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&n, _)| n)
        .collect();

    let mut sorted: Vec<String> = Vec::with_capacity(graph.len());

    while let Some(node) = topo_queue.pop_front() {
        sorted.push(node.to_owned());
        if let Some(deps) = graph.get(node) {
            for dep in deps {
                if let Some(deg) = in_degree.get_mut(dep.as_str()) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        topo_queue.push_back(dep.as_str());
                    }
                }
            }
        }
    }

    if sorted.len() != graph.len() {
        let cycle_nodes: Vec<&str> = graph
            .keys()
            .filter(|k| !sorted.contains(*k))
            .map(|s| s.as_str())
            .collect();
        let cycle_set: std::collections::HashSet<&str> = cycle_nodes.iter().copied().collect();
        let mut cycle_path: Vec<String> = Vec::new();
        if let Some(&start) = cycle_nodes.first() {
            let mut stack: Vec<&str> = vec![start];
            let mut visited: Vec<&str> = Vec::new();
            'dfs: loop {
                let Some(&cur) = stack.last() else { break };
                if let Some(pos) = visited.iter().position(|&v| v == cur) {
                    cycle_path = visited[pos..].iter().map(|s| s.to_string()).collect();
                    cycle_path.push(cur.to_string());
                    break 'dfs;
                }
                visited.push(cur);
                let mut found_next = false;
                if let Some(deps) = graph.get(cur) {
                    for dep in deps {
                        if cycle_set.contains(dep.as_str()) {
                            stack.push(dep.as_str());
                            found_next = true;
                            break;
                        }
                    }
                }
                if !found_next {
                    break;
                }
            }
        }
        return if cycle_path.is_empty() {
            Err(format!(
                "circular dependency detected involving: {}",
                cycle_nodes.join(", ")
            ))
        } else {
            Err(format!(
                "circular dependency detected: {}",
                cycle_path.join(" → ")
            ))
        };
    }

    let mut modules: HashMap<String, FunctionProto> = HashMap::new();
    modules.insert(canonical_entry.clone(), entry_proto.clone());

    for module_path in sorted.into_iter().rev() {
        if module_path == canonical_entry {
            continue;
        }
        let Some((_, program)) = node_sources.get(&module_path) else {
            continue;
        };

        let check = varn_checker::Checker::check(program);
        let exports = if program.filename.starts_with("std:")
            || program.filename.starts_with("core:")
            || program.filename.starts_with("runtime:")
        {
            varn_checker::module_resolver::resolve_stdlib_module_exports_ref(&program.filename)
        } else {
            varn_checker::module_resolver::resolve_module_exports_ref(
                &program.filename,
                &mut vec![],
            )
        };
        let mut export_names: Vec<std::rc::Rc<str>> = exports
            .keys()
            .map(|k| std::rc::Rc::from(k.as_str()))
            .collect();
        export_names.sort();
        let module_proto = varn_opt::compile_module(
            program,
            &check.type_annotations,
            &check.extension_calls,
            &check.extension_members,
            &check.extension_set_members,
            export_names,
        )
        .map_err(|e| format!("compile error in '{module_path}': {e}"))?;

        modules.insert(module_path, module_proto);
    }

    Ok(ModuleGraphBuild {
        entry_path: canonical_entry,
        modules,
        source_hashes,
        package_nodes: package_nodes.into_values().collect(),
        deps: graph,
    })
}

fn read_module_source(module_path: &str) -> Result<String, String> {
    if matches!(
        varn_core::ImportSpecifier::parse(module_path),
        varn_core::ImportSpecifier::Stdlib(_)
            | varn_core::ImportSpecifier::Core(_)
            | varn_core::ImportSpecifier::Runtime(_)
    ) {
        let provider = varn_modules::provider::get()
            .ok_or_else(|| "stdlib provider not registered".to_owned())?;
        return provider
            .embedded_source(module_path)
            .map(|s| s.to_owned())
            .or_else(|| {
                provider
                    .source_path(module_path)
                    .and_then(|p| std::fs::read_to_string(p).ok())
            })
            .ok_or_else(|| format!("stdlib source not found: {module_path}"));
    }
    std::fs::read_to_string(module_path).map_err(|e| e.to_string())
}

pub fn resolve_import_specifier(
    specifier: &str,
    module_dir: &Path,
    package_nodes: &mut HashMap<String, PackageNode>,
) -> Result<Option<String>, String> {
    use varn_core::ImportSpecifier;

    match ImportSpecifier::parse(specifier) {
        ImportSpecifier::Stdlib(_) | ImportSpecifier::Core(_) => {
            let provider = varn_modules::provider::get()
                .ok_or_else(|| "stdlib provider not registered".to_owned())?;
            if provider.embedded_source(specifier).is_some() {
                return Ok(Some(specifier.to_owned()));
            }
            if provider.source_path(specifier).is_some() {
                return Ok(Some(specifier.to_owned()));
            }
            if provider.bytecode_blob(specifier).is_some() {
                return Ok(Some(specifier.to_owned()));
            }
            Ok(None)
        }

        ImportSpecifier::Runtime(_) => Ok(Some(specifier.to_owned())),
        ImportSpecifier::Relative(rel) => {
            let joined = module_dir.join(&rel);
            if joined.exists() {
                if let Ok(canonical) = std::fs::canonicalize(&joined) {
                    return Ok(Some(varn_modules::normalize_path_string(
                        canonical.to_string_lossy().into_owned(),
                    )));
                }
                return Ok(Some(varn_modules::normalize_path_string(
                    joined.to_string_lossy().into_owned(),
                )));
            }
            Ok(None)
        }
        ImportSpecifier::Package(_) => {
            let resolved = varn_modules::resolve_pkg_specifier_detailed(module_dir, specifier)?;
            let integrity = std::fs::read(&resolved.resolved_path)
                .map(|bytes| format!("{:016x}", fnv1a64(&bytes)))
                .unwrap_or_else(|_| UNKNOWN_INTEGRITY_HASH.to_owned());
            package_nodes
                .entry(specifier.to_owned())
                .or_insert(PackageNode {
                    specifier: resolved.specifier.clone(),
                    package: resolved.package.clone(),
                    version: resolved.version.clone(),
                    subpath: resolved.subpath.clone(),
                    resolved_path: resolved.resolved_path.clone(),
                    integrity,
                });
            Ok(Some(resolved.resolved_path))
        }
    }
}
