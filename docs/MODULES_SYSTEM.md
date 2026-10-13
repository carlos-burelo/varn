# Sistema de módulos

Fuentes en `crates/varn-modules/src/`, `crates/varn-resolver/src/graph.rs`, `crates/varn-sem/src/resolver.rs`. Código manda.

## Provenance (`loader.rs:9-17`)

```rust
pub enum Provenance { Memory, File(PathBuf), Bundle, Native }
```

- `Memory`: `MemoryLoader` (`loader.rs:258-288`), mapa en memoria; gana al filesystem en registry.
- `File(PathBuf)`: `FilesystemLoader` solo `ModuleId::Local` (`loader.rs:234-256`); `ProviderLoader` lee `source_path(spec)` a `File(path)` (`loader.rs:143-158`).
- `Bundle`: `ProviderLoader` sirve `embedded_source` o `bundled_source` como `Bundle` (`loader.rs:134-141`).
- `Native`: variante declarada; sin constructor en `loader.rs`.

`ModuleLoader::resolve` default va por `crate::resolver::ModuleResolver` (`loader.rs:94-105`). `ModuleRegistry` prueba backends en orden, salta `NotFound` (`loader.rs:201-230`). `default_registry`: `FilesystemLoader + ProviderLoader` (`loader.rs:163-167`).

## Bundle `.vnb` (`bundle.rs:22-34`)

```rust
pub fn write_bundle(bundle: &StdBundle) -> Vec<u8>   // postcard + write_artifact(StdBundle, Distributable)
pub fn read_bundle(bytes: &[u8]) -> Result<StdBundle, String>  // read_artifact + postcard::from_bytes
```

`StdBundle { std_version, host_api_version: u32, modules: Vec<BundleModule> }`; `BundleModule { id, pure, interface, bytecode, source }` (`bundle.rs:6-20`). Compat: `validate_compat_with` compara `host_api_version` (`bundle.rs:37-47`).

## Raíz std (`std_root.rs:5-8`)

```rust
pub const STD_BUNDLE_FILE: &str = "std.vnb";
pub const STD_EMBEDDED_SENTINEL: &str = "@embedded";
```

`ENV_VARN_STD="VARN_STD"`, `STD_MANIFEST_FILE="std.json"`, `STD_DIR_NAME="std"` (`std_root.rs:3-6`). Resolución (`std_root.rs:62-87`): override proyecto → `VARN_STD=@embedded` = Embedded/Env → `classify(path)` = SourceTree/Env → dev-checkout (exe → ancestros + `std/`) → Embedded.

## Artefactos commiteados

`git ls-files "*.vnb"`: 0 ficheros. Ningún `.vnb` en repo.

## ModuleGraph (`crates/varn-resolver/src/graph.rs:27-55`)

Caché en memoria: `binds: FxHashMap<String, Arc<BindResult>>`, `exports: FxHashMap<String, Arc<ExportMap>>`, además `programs`, `arenas`, `resolved_paths`, `reverse_deps` (`graph.rs:8-20`).

- `bind` / `insert_bind` (`graph.rs:27-33`), `exports` / `insert_exports` (`graph.rs:35-41`), `program` / `insert_program` (43-49), `arena` / `insert_arena` (51-57).
- `invalidate` por `reverse_deps`, `evict_heavy`, `clear` (`graph.rs:83-133`).

Tipos `BindResult` / `ExportMap` vienen de `varn_sem::bind` / `varn_sem::exports` (`graph.rs:5-6`).

## ImportResolver (`crates/varn-sem/src/resolver.rs:6-9`)

```rust
pub trait ImportResolver {
    fn module_bind(&self, abs_path: &str) -> Option<Arc<BindResult>>;
    fn module_exports(&self, abs_path: &str, visiting: &mut Vec<String>) -> Arc<ExportMap>;
    ...
}
```

Resto del trait (`resolver.rs:11-60`): `stdlib_bind`, `stdlib_exports`, `resolve_specifier`, `record_dep`, `core_exports`, `evict_heavy`, `graph_stats`, `core_members`, `find_bind_for_type` (default), `stdlib_specs`, `find_stdlib_bind` (default).
