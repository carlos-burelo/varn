# Arquitectura de la stdlib

Tres capas, de abajo arriba:

| Capa | Dónde | Qué es |
|---|---|---|
| `runtime:*` | `crates/varn-builtins/src/modules/runtime/<mod>/` | Nativos Rust con contrato `.vn` (`declare function …`) |
| `core:*` | `crates/varn-builtins/src/modules/core/` | Tipos y globales del lenguaje (métodos de `int`, `str`, `Array`, errores, decoradores) |
| `std:*` | `std/` | Biblioteca en Varn sobre `runtime:*`; es lo que importa el usuario |

La std se distribuye siempre como imagen `std.vnb` (ver
[`MODULES_SYSTEM.md`](MODULES_SYSTEM.md)); los nativos, ver
[`LBI_ARCHITECTURE.md`](LBI_ARCHITECTURE.md).

## `std/` (30 módulos, 129 archivos `.vn`)

| Módulo | Archivos |
|---|---|
| `cli` | `args`, `color`, `prompt`, `table` |
| `collections` | `list`, `lru`, `priority_queue`, `queue`, `stack` |
| `compress` | `gzip`, `tar`, `zip` |
| `crypto` | `hash`, `jwt`, `password`, `totp`, `uuid` |
| `db` | `redis` |
| `encoding` | `base32`, `base64`, `csv`, `hex`, `html`, `json`, `msgpack`, `toml` |
| `env` | `env` |
| `error` | `mod` |
| `ffi` | `library`, `pointer`, `types` |
| `fs` | `file`, `glob`, `ops`, `stats` |
| `gpu` | `buffer`, `device` |
| `http` | `auth`, `client`, `core`, `headers`, `mime`, `multipart`, `request`, `response`, `route`, `server`, `status` |
| `io` | `stream` |
| `log` | `level`, `logger` |
| `markdown` | `ast`, `inline`, `parser`, `renderer`, `utils` |
| `math` | `class`, `constants`, `intrinsics`, `stats` |
| `net` | `encoding`, `ip`, `query`, `socket`, `udp`, `url` |
| `path` | `utils` |
| `pm` | `semver` |
| `process` | `child` |
| `reflect` | `introspect`, `meta` |
| `regex` | `regex` |
| `sqlite` | `db` |
| `sys` | `args`, `env` |
| `task` | `channel`, `combinators`, `cron`, `group`, `sync` |
| `test` | `assert`, `suite` |
| `text` | `shape` |
| `time` | `core`, `datetime`, `duration`, `instant` |
| `ui` | `catalog`, `component`, `edit`, `focus`, `frame`, `keys`, `layout`, `signal`, `style`, `widgets`, `window` |
| `ws` | `client`, `server` |

Cada módulo tiene además su `mod.vn`, que reexporta la superficie pública.

## Nativos (`varn-builtins/src/modules/mod.rs`)

26 módulos registrados en `native_modules!`: `buffer`, `compress`, `crypto`, `csv`,
`errors`, `ffi`, `fs`, `globals`, `gpu`, `io`, `json`, `math`, `net`, `path`,
`process`, `reflect`, `regex`, `sqlite`, `sys`, `task`, `testing` (sin entradas
nativas), `text`, `time`, `types` (`array`, `bigint`, `bool`, `bytes`, `char`,
`decimal`, `float`, `int`, `iteration`, `map`, `range`, `set`, `string`), `ui`, `ws`.

## GPU (`varn-gpu` + `runtime:gpu` + `std:gpu`)

- `varn-gpu`: arena de buffers con handles generacionales (`arena.rs`), grafo perezoso `Add/Mul/Scale/MatMul` evaluado al descargar (`graph.rs`), sonda de adaptadores `wgpu` (`backend.rs`) y ejecución WGSL con un `submit` por grafo y fallback a CPU ante cualquier error (`execute.rs`).
- Modelo de costo: `GPU_MIN_MACS = 200_000` (`lib.rs`); por debajo calcula la CPU. `engine()` informa qué motor resolvió la última operación.
- `std:gpu`: `device.vn` (`backend`, `deviceCount`, `adapterName`, `engine`, `isAvailable`, `getInfo`) y `buffer.vn` (`GpuBuffer` con `add/mul/scale/dot/matmul/download/shape/reshape/dispose` + `create/zeros/upload`).
- Puerta: `tests/214-gpu-compute.vn`; bench `tests/benchmarks/vn/bench_gpu_matmul.vn`.

## UI (`varn-ui` + `runtime:ui` + `std:ui`)

- `varn-ui`: ventanas `winit` + render `wgpu` (`window.rs`, `render.rs`), eventos (`events.rs`), compositor de quads instanciados con atlas de glifos (`comp.rs`, `quad.rs`, `pipes.rs`, `present.rs`).
- `runtime:ui`: ventanas como handles `int` con cola de eventos y lista de comandos de dibujo (`Clear/Rect/Text`). Headless por defecto; `VARN_UI_REAL=1` abre ventanas reales. Toda la capa está tras la capability `ui` (`--allow-ui`).
- `std:ui`, dos niveles:
  - **Inmediato**: `UiWindow`/`UiEvent` (`window`), `UiFrame` (`frame`: flancos de ratón/teclas), `Focus`, `TextEdit` (`edit`), `layout` (`pad/row/col`, `textX/textY`, `ellipsize`), `keys`, y widgets `Label/Box/Column/Row/Button/Field/Spacer/Divider/Checkbox/List` con `render` y `run` (`widgets`).
  - **Declarativo** (objetivo de [`UI-SPEC.md`](UI-SPEC.md)): señales `signal/computed/effect/batch/untracked` (`signal`), `component`/`memo` (`component`), nodos `VNode` y catálogo `column/row/stack/padding/sizedBox/spacer/text/button/input/image/progressIndicator/container/animated/when/each` (`catalog`), estilos y temas (`style`).
- Puertas: `tests/215-ui-headless.vn`, `217-ui-capabilities.vn`, `219-widgets.vn`, `220-ui-declarative.vn`. Demos: `examples/ui-demo.vn`, `examples/ui-declarative-demo.vn`.

## Texto (`varn-text` + `runtime:text` + `std:text`)

- `varn-text`: base de fuentes del sistema cargada una vez (`font.rs`), shaping con `swash` (`shape.rs`), atlas de glifos rasterizados (`atlas.rs`).
- `std:text` (`shape.vn`): `fontCount`, `fonts`, `shape`, `measure`, `rasterGlyph`, `glyphCacheSize`, `ascent`, `descent`, `wrap`.
- Los ids de glifo dependen de la fuente del sistema: los tests son estructurales. Puerta: `tests/216-text-shaping.vn`.

## Macros

`varn-macros` es el único crate proc-macro: `varn_contract!` (`src/varn_contract/`,
`contract_parse.rs` parsea el `.vn` con el parser real), `#[jit_slow]` y
`jit_helper_table!`. Lo consumen `varn-builtins` (contratos) y `varn-vm` (helpers JIT).
