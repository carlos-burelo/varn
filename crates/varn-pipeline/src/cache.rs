use super::compile::CompileOutput;
use crate::PipelineError;
use rustc_hash::FxHashMap;
use std::path::Path;
use std::rc::Rc;
use varn_types::ModuleGraphArtifact;

type PipelineResult<T> = Result<T, PipelineError>;

pub fn compile_cache_path(file_path: &str) -> std::path::PathBuf {
    let path = Path::new(file_path);
    let file_dir = path.parent().unwrap_or_else(|| Path::new("."));

    let project_root = varn_modules::artifact::find_project_root(file_dir);

    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path_hash = crate::hash::fnv1a64(canonical.to_string_lossy().as_bytes());

    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    // La clave del artefacto va en el NOMBRE, no sólo dentro de la envolvente:
    // si dos binarios distintos comparten ruta de caché se sobreescriben el
    // archivo mutuamente y ninguno llega a reutilizar nada. Separarlos por
    // nombre deja a cada uno su propia entrada.
    //
    // Extensión `.vncache`, no `.vnc`: esta última es la del artefacto que
    // produce `vn build` y que el usuario ejecuta. Compartirla hacía que el
    // mismo sufijo nombrara dos cosas con reglas de validez opuestas —
    // portable una, atada a este binario la otra.
    varn_modules::artifact::get_bytecode_cache_dir(&project_root).join(format!(
        "{}.{:x}.{:08x}.vncache",
        stem,
        path_hash as u32,
        varn_modules::artifact::cache_key()
    ))
}

/// Carga el grafo cacheado si sigue siendo válido.
///
/// `verbose` hace visible el MOTIVO de un fallo, no sólo el hecho. Un miss
/// silencioso se comporta igual que una caché fría, así que una validación
/// rota se manifiesta como lentitud difusa y puede sobrevivir indefinidamente
/// sin que nadie la note: exactamente lo que pasó con la regla que clasificaba
/// `std:time/duration` como ruta de disco.
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
            let current_hash = crate::hash::fnv1a64(entry_source.as_bytes());
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
        // Procedencia decidida por el provider, no por la forma del texto.
        // La regla anterior era `contiene ':' y no contiene '/'`, que clasifica
        // `std:time` como virtual pero `std:time/duration` como ruta de disco:
        // el segundo caía al `fs::read` de una ruta inexistente y el grafo
        // entero fallaba la validación en CADA arranque. Un solo id con
        // submódulo basta para que un programa no vuelva a acertar la caché.
        if let Some(provider) = varn_modules::provider::get() {
            if let Some(src) = provider.embedded_source(path) {
                let current_hash = crate::hash::fnv1a64(src.as_bytes());
                if cached_hash != current_hash {
                    reason(format_args!("el módulo embebido {path} cambió"));
                    return Ok(None);
                }
                continue;
            }
            if let Some(p) = provider.source_path(path) {
                // std servida desde un árbol de fuentes: revalidar contra disco
                // para que editar std/*.vn invalide las cachés dependientes.
                match std::fs::read(&p) {
                    Ok(src) => {
                        let current_hash = crate::hash::fnv1a64(&src);
                        if cached_hash != current_hash {
                            reason(format_args!("la fuente std {path} cambió"));
                            return Ok(None);
                        }
                        continue;
                    }
                    Err(e) => {
                        reason(format_args!("la fuente std {path} no se pudo leer: {e}"));
                        return Ok(None);
                    }
                }
            }
        }

        match std::fs::read(path) {
            Ok(src) => {
                let current_hash = crate::hash::fnv1a64(&src);
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
            PipelineError::fatal(format!(
                "{}{}error[cache]{}: cannot create cache dir: {}",
                varn_debug::colors::BOLD,
                varn_debug::colors::C_ERRORS,
                varn_debug::colors::R,
                e
            ))
        })?;
    }
    let payload = postcard::to_allocvec(graph).map_err(|e| {
        PipelineError::fatal(format!(
            "{}{}error[cache]{}: serialize failed: {}",
            varn_debug::colors::BOLD,
            varn_debug::colors::C_ERRORS,
            varn_debug::colors::R,
            e
        ))
    })?;
    let bytes = varn_modules::artifact::write_artifact(
        varn_modules::artifact::ArtifactKind::ModuleGraph,
        varn_modules::artifact::ArtifactClass::Cache,
        &payload,
    );
    std::fs::write(cache_path, bytes).map_err(|e| {
        PipelineError::fatal(format!(
            "{}{}error[cache]{}: write failed: {}",
            varn_debug::colors::BOLD,
            varn_debug::colors::C_ERRORS,
            varn_debug::colors::R,
            e
        ))
    })?;
    varn_modules::artifact::prune_superseded(cache_path);
    Ok(())
}

pub fn compile_output_from_graph(graph: ModuleGraphArtifact) -> PipelineResult<CompileOutput> {
    let entry_path = graph.entry_path.clone();
    let entry_proto = graph.entry_proto().cloned().ok_or_else(|| {
        PipelineError::fatal(format!(
            "{}{}error[cache]{}: entry module not found in graph",
            varn_debug::colors::BOLD,
            varn_debug::colors::C_ERRORS,
            varn_debug::colors::R
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
