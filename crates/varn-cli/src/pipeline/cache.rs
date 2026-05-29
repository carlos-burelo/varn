use super::compile::CompileOutput;
use crate::error::CliError;
use rustc_hash::FxHashMap;
use std::path::Path;
use std::rc::Rc;
use varn_types::ModuleGraphArtifact;

type PipelineResult<T> = Result<T, CliError>;

pub fn compile_cache_path(file_path: &str) -> std::path::PathBuf {
    let path = Path::new(file_path);
    let file_dir = path.parent().unwrap_or_else(|| Path::new("."));

    let project_root = file_dir
        .ancestors()
        .find(|dir| dir.join("varn.json").exists())
        .unwrap_or(file_dir);

    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path_hash = super::hash::fnv1a64(canonical.to_string_lossy().as_bytes());

    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    project_root
        .join(".vn")
        .join("cache")
        .join(format!("{}.{:x}.bin", stem, path_hash as u32))
}

pub fn load_cached_graph(
    cache_path: &Path,
    version: u32,
    entry_source: &str,
) -> Result<Option<ModuleGraphArtifact>, String> {
    if !cache_path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(cache_path).map_err(|e| e.to_string())?;
    let graph: ModuleGraphArtifact = postcard::from_bytes(&bytes).map_err(|e| e.to_string())?;

    if graph.format_version != version {
        return Ok(None);
    }

    if let Some(&cached_hash) = graph.source_hashes.get(&graph.entry_path) {
        let current_hash = crate::pipeline::hash::fnv1a64(entry_source.as_bytes());
        if cached_hash != current_hash {
            return Ok(None);
        }
    }

    // Validate all file-based dependencies.
    for (path, &cached_hash) in &graph.source_hashes {
        if path == &graph.entry_path {
            continue;
        }
        // Check stdlib/core/runtime modules against their embedded source.
        if path.contains(':') && !path.contains('/') && !path.contains('\\') {
            if let Some(spec) = varn_builtins::spec_for(path) {
                if let Some(src) = spec.source() {
                    let current_hash = crate::pipeline::hash::fnv1a64(src.as_bytes());
                    if cached_hash != current_hash {
                        return Ok(None);
                    }
                }
            }
            continue;
        }
        if let Ok(src) = std::fs::read(path) {
            let current_hash = crate::pipeline::hash::fnv1a64(&src);
            if cached_hash != current_hash {
                return Ok(None);
            }
        }
    }

    Ok(Some(graph))
}

pub fn store_cached_graph(cache_path: &Path, graph: &ModuleGraphArtifact) -> PipelineResult<()> {
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            CliError::fatal(format!(
                "{}{}error[cache]{}: cannot create cache dir: {}",
                varn_debug::colors::BOLD,
                varn_debug::colors::C_ERRORS,
                varn_debug::colors::R,
                e
            ))
        })?;
    }
    let bytes = postcard::to_allocvec(graph).map_err(|e| {
        CliError::fatal(format!(
            "{}{}error[cache]{}: serialize failed: {}",
            varn_debug::colors::BOLD,
            varn_debug::colors::C_ERRORS,
            varn_debug::colors::R,
            e
        ))
    })?;
    std::fs::write(cache_path, bytes).map_err(|e| {
        CliError::fatal(format!(
            "{}{}error[cache]{}: write failed: {}",
            varn_debug::colors::BOLD,
            varn_debug::colors::C_ERRORS,
            varn_debug::colors::R,
            e
        ))
    })?;
    Ok(())
}

pub fn compile_output_from_graph(graph: ModuleGraphArtifact) -> PipelineResult<CompileOutput> {
    let entry_path = graph.entry_path.clone();
    let entry_proto = graph.entry_proto().cloned().ok_or_else(|| {
        CliError::fatal(format!(
            "{}{}error[cache]{}: entry module not found in graph",
            varn_debug::colors::BOLD,
            varn_debug::colors::C_ERRORS,
            varn_debug::colors::R
        ))
    })?;

    let mut precompiled: FxHashMap<varn_core::ModuleId, Rc<varn_compiler::FunctionProto>> = FxHashMap::default();
    for (path, proto) in &graph.modules {
        if *path == entry_path {
            continue;
        }
        precompiled.insert(varn_core::ModuleId::from_canonical_str(path), Rc::new(proto.clone()));
    }

    Ok(CompileOutput {
        entry_proto,
        precompiled: Rc::new(precompiled),
        graph_artifact: graph,
    })
}
