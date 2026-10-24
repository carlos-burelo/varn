use super::compile::CompileOutput;
use crate::Failure;
use rustc_hash::FxHashMap;
use std::path::Path;
use std::rc::Rc;
use varn_types::ModuleGraphArtifact;

type PipelineResult<T> = Result<T, Failure>;

pub fn compile_cache_path(file_path: &str) -> std::path::PathBuf {
    let path = Path::new(file_path);
    let file_dir = path.parent().unwrap_or_else(|| Path::new("."));

    let project_root = varn_modules::artifact::find_project_root(file_dir);

    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path_hash = varn_core::fingerprint::fingerprint(canonical.to_string_lossy().as_bytes());

    let stem = path.file_stem().unwrap_or_default().to_string_lossy();

    let std_identity = varn_modules::provider::get().map_or(0, |p| p.image_identity());
    varn_modules::artifact::get_bytecode_cache_dir(&project_root).join(format!(
        "{}.{:x}.{:016x}.vncache",
        stem,
        path_hash as u32,
        u64::from(varn_modules::artifact::cache_key()) ^ std_identity
    ))
}

pub fn load_cached_graph(
    cache_path: &Path,
    entry_source: &str,
    verbose: bool,
) -> Result<Option<ModuleGraphArtifact>, String> {
    if !cache_path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(cache_path).map_err(|e| e.to_string())?;
    let payload = varn_modules::artifact::read_artifact(
        varn_modules::artifact::ArtifactKind::ModuleGraph,
        &bytes,
    )
    .map_err(|e| e.to_string())?;
    let reason = |what: std::fmt::Arguments<'_>| {
        if verbose {
            varn_core::term::terminal::tagged("Varn", format_args!("cache miss: {what}"));
        }
    };

    let graph: ModuleGraphArtifact = postcard::from_bytes(payload).map_err(|e| e.to_string())?;

    match graph.source_hashes.get(&graph.entry_path) {
        Some(&cached_hash) => {
            let current_hash = varn_core::fingerprint::fingerprint(entry_source.as_bytes());
            if cached_hash != current_hash {
                reason(format_args!("el fuente de entrada cambió"));
                return Ok(None);
            }
        }
        None => {
            reason(format_args!("el artefacto no registra hash de la entrada"));
            return Ok(None);
        }
    }

    for (path, &cached_hash) in &graph.source_hashes {
        if path == &graph.entry_path {
            continue;
        }

        match varn_modules::loader::read_module_text(path) {
            Ok(src) => {
                let current_hash = varn_core::fingerprint::fingerprint(src.as_bytes());
                if cached_hash != current_hash {
                    reason(format_args!("la dependencia {path} cambió"));
                    return Ok(None);
                }
            }
            Err(e) => {
                reason(format_args!("la dependencia {path} no se pudo leer: {e}"));
                return Ok(None);
            }
        }
    }

    Ok(Some(graph))
}

pub fn store_cached_graph(cache_path: &Path, graph: &ModuleGraphArtifact) -> PipelineResult<()> {
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            Failure::internal(format!(
                "{}{}error[cache]{}: cannot create cache dir: {}",
                varn_core::term::colors::BOLD,
                varn_core::term::colors::C_ERRORS,
                varn_core::term::colors::R,
                e
            ))
        })?;
    }
    let payload = postcard::to_allocvec(graph).map_err(|e| {
        Failure::internal(format!(
            "{}{}error[cache]{}: serialize failed: {}",
            varn_core::term::colors::BOLD,
            varn_core::term::colors::C_ERRORS,
            varn_core::term::colors::R,
            e
        ))
    })?;
    let bytes = varn_modules::artifact::write_artifact(
        varn_modules::artifact::ArtifactKind::ModuleGraph,
        varn_modules::artifact::ArtifactClass::Cache,
        &payload,
    );
    varn_modules::artifact::write_artifact_file(cache_path, &bytes).map_err(|e| {
        Failure::internal(format!(
            "{}{}error[cache]{}: write failed: {}",
            varn_core::term::colors::BOLD,
            varn_core::term::colors::C_ERRORS,
            varn_core::term::colors::R,
            e
        ))
    })?;
    varn_modules::artifact::prune_superseded(cache_path);
    Ok(())
}

pub fn compile_output_from_graph(graph: ModuleGraphArtifact) -> PipelineResult<CompileOutput> {
    let entry_path = graph.entry_path.clone();
    let entry_proto = graph.entry_proto().cloned().ok_or_else(|| {
        Failure::internal(format!(
            "{}{}error[cache]{}: entry module not found in graph",
            varn_core::term::colors::BOLD,
            varn_core::term::colors::C_ERRORS,
            varn_core::term::colors::R
        ))
    })?;

    let mut precompiled: FxHashMap<varn_core::ModuleId, Rc<varn_compiler::FunctionProto>> =
        FxHashMap::default();
    for (path, proto) in &graph.modules {
        if *path == entry_path {
            continue;
        }
        precompiled.insert(
            varn_core::ModuleId::from_canonical_str(path),
            Rc::new(proto.clone()),
        );
    }

    Ok(CompileOutput {
        entry_proto,
        precompiled: Rc::new(precompiled),
        graph_artifact: graph,
    })
}
