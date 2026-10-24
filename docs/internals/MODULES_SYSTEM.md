# Sistema de módulos

Fuentes: `crates/varn-modules/src/`, `crates/varn-cli/src/std_source.rs`,
`crates/varn-builtins/src/provider_impl.rs`, `crates/varn-resolver/src/graph.rs`,
`crates/varn-sem/src/resolver.rs`. Código manda.

## Identidades

`ModuleId` (`varn-core/src/module_id.rs`): `Local(path)`, `Package { .. }`, `Std(spec)`, `Core(spec)`,
`Runtime(spec)`. Los especificadores `std:x`, `core:x` y `runtime:x` nunca tocan el
disco del usuario: los sirve el proveedor de la std. Un solo resolvedor de
especificadores (`resolver.rs`) normaliza rutas relativas y esquemas.

## Carga de fuente (`loader.rs`)

```rust
pub trait ModuleLoader: Send + Sync {
    fn resolve(&self, specifier: &str, from: &ModuleId) -> Result<ModuleId, LoadError>; // por defecto: resolver::resolve
    fn source(&self, id: &ModuleId) -> Result<ModuleSource, LoadError>;
}
```

- `FilesystemLoader`: solo `ModuleId::Local`.
- `ProviderLoader`: `std:`/`core:`/`runtime:` vía el `StdlibProvider` registrado (`provider.rs`). Si el proveedor tiene el módulo en la imagen, devuelve fuente + interfaz + bytecode (`Provenance::Bundle`); si solo conoce una ruta, la lee (`Provenance::File`).
- `MemoryLoader`: mapa en memoria (LSP, tests).
- `ModuleRegistry`: prueba backends en orden y salta `NotFound`. `default_registry()` = `FilesystemLoader` + `ProviderLoader`.

`Provenance { Memory, File(PathBuf), Bundle, Native }`.

## La std es siempre una imagen

`std` y los módulos `core:` se consumen siempre desde una imagen `std.vnb`
(`std_image.rs`): cabecera + tabla de módulos (`id`, `path`, `pure`, `interface`,
`bytecode`, `source`) indexada sin copiar. `StdImage::parse` valida el sobre del
artefacto; `write_image` la produce (`vn std-bundle`).

Resolución de la raíz (`std_root.rs::resolve`, una vez por proceso):

1. clave `std` del manifiesto del proyecto (`ProjectOverride`);
2. `VARN_STD` = ruta a un árbol fuente, o `@embedded` (`Env`);
3. un `std/` en un ancestro del ejecutable (`DevCheckout`);
4. imagen embebida (`Embedded`).

Qué imagen se usa (`varn-cli/src/std_source.rs::select`):

- **Árbol fuente** (override, `VARN_STD=<ruta>` o dev-checkout): `vn` construye la imagen en `<cache>/std/std.<huella>.vnb` lanzando `vn std-bundle` como proceso hijo. La huella cubre el binario y ruta/tamaño/mtime de cada módulo, así que cambiar `std/` o recompilar `vn` la regenera. Un error de tipos en `std/` aparece en la primera corrida, no en `cargo check`.
- **Embebida**: con la feature `embed-std`, `dist/std.vnb` va dentro del binario; si no, se lee de `VARN_STD_BUNDLE`.

El checker lee las interfaces de std/core directamente de la imagen; no recompila la std.

## Grafo y caché de interfaces

`ModuleGraph` (`varn-resolver/src/graph.rs`): caché en memoria por ruta de
`BindResult`, `ExportMap`, `Program`, `AstArena`, rutas resueltas y dependencias
inversas (`invalidate` por `reverse_deps`, `evict_heavy`, `clear`).

`ImportResolver` (`varn-sem/src/resolver.rs`) es lo que ve el checker:
`module_bind`, `module_exports`, `stdlib_bind`, `stdlib_exports`,
`resolve_specifier`, `record_dep`, `core_exports`, `core_members`, `evict_heavy`,
`graph_stats`, `find_bind_for_type`, `stdlib_specs`, `find_stdlib_bind`.

Los artefactos en disco (grafo compilado, interfaces) usan el sobre de
`artifact.rs` y viven en la caché global (`~/.vn/cache`, `VARN_HOME`,
`VARN_CACHE_DIR`). Ningún `.vnb` está versionado en el repo (`dist/` se ignora).

## Módulos nativos

`runtime:*` y la parte nativa de `core:*` tienen un contrato `.vn` y una
implementación Rust en `varn-builtins`. Ver [`LBI_ARCHITECTURE.md`](LBI_ARCHITECTURE.md).
