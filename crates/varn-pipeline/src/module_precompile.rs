use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::collections::VecDeque;
use std::path::Path;
use varn_core::fingerprint::fingerprint;

use varn_compiler::FunctionProto;
use varn_core::ast::{AstArena, Program};
use varn_types::PackageNode;

const UNKNOWN_INTEGRITY_HASH: &str = "0000000000000000";

pub struct ModuleGraphBuild {
    pub entry_path: String,
    pub modules: HashMap<String, FunctionProto>,
    pub source_hashes: HashMap<String, u64>,
    pub package_nodes: Vec<varn_types::PackageNode>,

    pub deps: HashMap<String, Vec<String>>,
    pub class_tables: HashMap<String, crate::class_link::ModuleClasses>,
    pub warnings: Vec<crate::DiagnosticReport>,
}

struct Discovered {
    canonical_entry: String,
    graph: HashMap<String, Vec<String>>,
    source_hashes: HashMap<String, u64>,
    node_sources: HashMap<String, (String, Program, AstArena, varn_core::AtomInterner)>,
    package_nodes: HashMap<String, PackageNode>,
    sorted: Vec<String>,
}

fn discover(
    entry_program: &Program,
    entry_arena: &AstArena,
    entry_source: &str,
    entry_path: &str,
    entry_interner: &varn_core::AtomInterner,
) -> Result<Discovered, crate::Failure> {
    let canonical_entry = varn_modules::canonical_or_original(Path::new(entry_path));

    let mut graph: HashMap<String, Vec<String>> = HashMap::default();
    let mut source_hashes: HashMap<String, u64> = HashMap::default();
    source_hashes.insert(
        canonical_entry.clone(),
        fingerprint(entry_source.as_bytes()),
    );

    let mut node_sources: HashMap<String, (String, Program, AstArena, varn_core::AtomInterner)> =
        HashMap::default();
    let mut package_nodes: HashMap<String, PackageNode> = HashMap::default();

    let entry_dir = Path::new(&canonical_entry)
        .parent()
        .unwrap_or_else(|| Path::new("."));

    let mut entry_deps = Vec::new();
    for spec in crate::import_collector::collect_imports(entry_program, entry_arena, entry_interner)
    {
        if let Some(dep_path) = resolve_import_specifier(&spec, entry_dir, &mut package_nodes)
            .map_err(|e| format!("{e}\n  imported from: {entry_path}"))?
        {
            entry_deps.push(dep_path);
        }
    }
    graph.insert(canonical_entry.clone(), entry_deps.clone());

    let mut queue: VecDeque<String> = entry_deps.into_iter().collect();
    let mut enqueued: HashSet<String> = HashSet::default();
    enqueued.insert(canonical_entry.clone());

    while let Some(module_path) = queue.pop_front() {
        if enqueued.contains(&module_path) {
            continue;
        }
        enqueued.insert(module_path.clone());

        if let Some(provider) = varn_modules::provider::get()
            && provider.module(&module_path).is_some()
        {
            graph.insert(module_path.clone(), Vec::new());
            continue;
        }

        let source = read_module_source(&module_path)
            .map_err(|e| format!("cannot read module '{module_path}': {e}"))?;
        source_hashes.insert(module_path.clone(), fingerprint(source.as_bytes()));

        let (program, arena, interner) = crate::quiet_parse::parse_module(&source, &module_path)?;

        let module_dir = Path::new(&module_path)
            .parent()
            .unwrap_or_else(|| Path::new("."));

        let mut deps = Vec::new();
        for child_spec in crate::import_collector::collect_imports(&program, &arena, &interner) {
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
        node_sources.insert(module_path, (source, program, arena, interner));
    }

    let mut in_degree: HashMap<&str, usize> = HashMap::default();
    for (node, deps) in &graph {
        in_degree.entry(node.as_str()).or_insert(0);
        for dep in deps {
            if graph.contains_key(dep.as_str()) {
                *in_degree.entry(dep.as_str()).or_insert(0) += 1;
            }
        }
    }

    let mut topo_queue: VecDeque<&str> = {
        let mut zeros: Vec<&str> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(&n, _)| n)
            .collect();
        zeros.sort();
        zeros.into_iter().collect()
    };

    let mut sorted: Vec<String> = Vec::with_capacity(graph.len());

    while let Some(node) = topo_queue.pop_front() {
        sorted.push(node.to_owned());
        if let Some(deps) = graph.get(node) {
            let mut newly_ready: Vec<&str> = Vec::new();
            for dep in deps {
                if let Some(deg) = in_degree.get_mut(dep.as_str()) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        newly_ready.push(dep.as_str());
                    }
                }
            }
            newly_ready.sort();
            for r in newly_ready {
                topo_queue.push_back(r);
            }
        }
    }

    if sorted.len() != graph.len() {
        let mut cycle_nodes: Vec<&str> = graph
            .keys()
            .filter(|k| !sorted.contains(*k))
            .map(|s| s.as_str())
            .collect();
        cycle_nodes.sort();
        let cycle_set: rustc_hash::FxHashSet<&str> = cycle_nodes.iter().copied().collect();
        let mut cycle_path: Vec<String> = Vec::new();
        if let Some(&start) = cycle_nodes.first() {
            let mut stack: Vec<&str> = vec![start];
            let mut visited: Vec<&str> = Vec::new();
            'dfs: while let Some(&cur) = stack.last() {
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
            Err(crate::Failure::internal(format!(
                "circular dependency detected involving: {}",
                cycle_nodes.join(", ")
            )))
        } else {
            Err(crate::Failure::internal(format!(
                "circular dependency detected: {}",
                cycle_path.join(" → ")
            )))
        };
    }

    Ok(Discovered {
        canonical_entry,
        graph,
        source_hashes,
        node_sources,
        package_nodes,
        sorted,
    })
}

pub fn check_module_graph(
    entry_program: &Program,
    entry_arena: &AstArena,
    entry_source: &str,
    entry_path: &str,
    entry_interner: &varn_core::AtomInterner,
    session: &crate::ModuleResolver,
) -> Result<Vec<crate::DiagnosticReport>, crate::Failure> {
    let found = discover(
        entry_program,
        entry_arena,
        entry_source,
        entry_path,
        entry_interner,
    )?;
    let mut warnings: Vec<crate::DiagnosticReport> = Vec::new();
    for module_path in found.sorted.iter().rev() {
        if *module_path == found.canonical_entry {
            continue;
        }
        let Some((module_source, program, arena, interner)) = found.node_sources.get(module_path)
        else {
            continue;
        };
        let check = check_node(program, arena, interner, session);
        if let Some(report) =
            crate::check::warnings_of(&check.diagnostics, &program.filename, module_source)?
        {
            warnings.push(report);
        }
        crate::lower::check_exports(program, session)?;
    }
    Ok(warnings)
}

fn check_node(
    program: &Program,
    arena: &AstArena,
    interner: &varn_core::AtomInterner,
    session: &crate::ModuleResolver,
) -> varn_sem::output::CheckResult {
    varn_checker::Checker::check_with(
        program,
        arena,
        interner.clone(),
        session,
        varn_sem::output::CheckOptions::compile(),
    )
}

pub fn build_module_graph(
    entry_program: &Program,
    entry_arena: &AstArena,
    entry_source: &str,
    entry_path: &str,
    entry_proto: &FunctionProto,
    entry_interner: &varn_core::AtomInterner,
    session: &crate::ModuleResolver,
) -> Result<ModuleGraphBuild, crate::Failure> {
    let Discovered {
        canonical_entry,
        graph,
        source_hashes,
        node_sources,
        package_nodes,
        sorted,
    } = discover(
        entry_program,
        entry_arena,
        entry_source,
        entry_path,
        entry_interner,
    )?;

    let mut modules: HashMap<String, FunctionProto> = HashMap::default();
    modules.insert(canonical_entry.clone(), entry_proto.clone());
    let mut class_tables: HashMap<String, crate::class_link::ModuleClasses> = HashMap::default();
    let mut warnings: Vec<crate::DiagnosticReport> = Vec::new();

    for module_path in sorted.into_iter().rev() {
        if module_path == canonical_entry {
            continue;
        }
        let Some((module_source, program, arena, interner)) = node_sources.get(&module_path) else {
            continue;
        };

        let check = check_node(program, arena, interner, session);

        if let Some(report) =
            crate::check::warnings_of(&check.diagnostics, &program.filename, module_source)?
        {
            warnings.push(report);
        }
        let lowered = crate::lower::lower_module(crate::lower::LowerInput {
            program,
            arena,
            check: &check,
            resolver: session,
            source: module_source,
            measure: false,
        })
        .map_err(|e| crate::Failure::internal(format!("compile error in '{module_path}': {e}")))?;
        class_tables.insert(
            module_path.clone(),
            crate::class_link::ModuleClasses {
                classes: lowered.tir.classes.clone(),
                origins: lowered
                    .tir
                    .class_origins
                    .iter()
                    .map(|o| o.as_ref().map(|s| s.to_string()))
                    .collect(),
                types: lowered.tir.types.clone(),
            },
        );

        modules.insert(module_path, lowered.proto);
    }

    Ok(ModuleGraphBuild {
        entry_path: canonical_entry,
        modules,
        source_hashes,
        package_nodes: package_nodes.into_values().collect(),
        deps: graph,
        class_tables,
        warnings,
    })
}

fn read_module_source(module_path: &str) -> Result<String, String> {
    varn_modules::loader::read_module_text(module_path)
        .map(|text| text.to_string())
        .map_err(|e| e.to_string())
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
            let known = provider.module(specifier).is_some();
            Ok(known.then(|| specifier.to_owned()))
        }

        ImportSpecifier::Runtime(_) => Ok(Some(specifier.to_owned())),
        ImportSpecifier::Relative(_) => {
            match varn_modules::resolver::resolve_in(specifier, module_dir)? {
                varn_core::ModuleId::Local(p) if Path::new(p.as_ref()).exists() => {
                    Ok(Some(p.to_string()))
                }
                varn_core::ModuleId::Local(_)
                | varn_core::ModuleId::Core(_)
                | varn_core::ModuleId::Std(_)
                | varn_core::ModuleId::Runtime(_)
                | varn_core::ModuleId::Package { .. } => Ok(None),
            }
        }
        ImportSpecifier::Package(_) => {
            let resolved = varn_modules::resolve_pkg_specifier_detailed(module_dir, specifier)?;
            let integrity = std::fs::read(&resolved.resolved_path)
                .map(|bytes| format!("{:016x}", fingerprint(&bytes)))
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
